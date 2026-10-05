//! The parent side: plans a run, starts one child process per model, enforces
//! deadlines, samples memory and records every event so a run can resume.
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use speech_to_text_lib::{asr_scoring, storage, vocabulary};

use crate::corpus::{self, Clip};
use crate::engine::{ChildClip, ChildJob, Event, ModeSpec};
use crate::models::{self, BenchModel};
use crate::paths::{display, write_atomic, Paths};
use crate::report;
use crate::system::{self, Sampler};

pub const CHILD_ARG: &str = "run-one";
const LOAD_DEADLINE: Duration = Duration::from_secs(20 * 60);
const MAX_SESSIONS_PER_MODEL: usize = 4;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub models: Option<String>,
    pub category: Option<String>,
    pub languages: Option<String>,
    pub clip_filter: Option<String>,
    pub modes: Vec<String>,
    pub repeats: u32,
    pub long_repeats: u32,
    pub stream_repeats: u32,
    pub fixed_language: bool,
    pub quick: bool,
    pub synthetic: bool,
    pub include_unverified: bool,
    pub force_all_clips: bool,
    pub cooldown_seconds: u64,
    pub allow_battery: bool,
    pub allow_running_app: bool,
    pub seed: Option<u64>,
    pub resume: Option<PathBuf>,
    /// With `resume`: discard these models' results and measure them again.
    pub redo: Option<String>,
    pub export_summary: bool,
    pub open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub repeats: u32,
    pub long_repeats: u32,
    pub stream_repeats: u32,
    pub modes: Vec<String>,
    pub language_mode: String,
    pub synthetic: bool,
    pub quick: bool,
    pub cooldown_seconds: u64,
    pub force_all_clips: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub started_at: String,
    pub seed: u64,
    pub settings: Settings,
    pub machine: system::Machine,
    pub git_commit: Option<String>,
    pub git_dirty: Option<bool>,
    pub power: system::Power,
    pub thermal_at_start: Option<String>,
    pub corpus_path: String,
    pub models: Vec<BenchModel>,
    pub clips: Vec<Clip>,
    pub modes: Vec<ModeSpec>,
    pub vocabulary_term_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLine {
    pub model: String,
    pub event: Event,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLine {
    pub model: String,
    pub cold_ms: Option<u64>,
    pub warm_ms: Option<u64>,
    pub per_transcription: bool,
    pub peak_memory_bytes: u64,
    pub wired_delta_bytes: Option<u64>,
    pub error: Option<String>,
    pub skipped: Vec<(String, String)>,
}

pub struct RunDir {
    pub dir: PathBuf,
}

impl RunDir {
    pub fn plan(&self) -> PathBuf {
        self.dir.join("plan.json")
    }
    pub fn events(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }
    pub fn models(&self) -> PathBuf {
        self.dir.join("models.jsonl")
    }
    pub fn log(&self) -> PathBuf {
        self.dir.join("run.log")
    }
    pub fn load_plan(&self) -> Result<Plan> {
        let text = std::fs::read_to_string(self.plan())
            .with_context(|| format!("{} is not a benchmark run (no plan.json)", self.dir.display()))?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn load_events(&self) -> Vec<EventLine> {
        read_lines(&self.events())
    }
    pub fn load_models(&self) -> Vec<ModelLine> {
        read_lines(&self.models())
    }
}

fn read_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn append<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Deterministic shuffle (SplitMix64) so a seed reproduces the model order.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for index in (1..items.len()).rev() {
        items.swap(index, (next() % (index as u64 + 1)) as usize);
    }
}

pub fn child_clip(clip: &Clip, corpus: &Path) -> ChildClip {
    ChildClip {
        id: clip.id.clone(),
        wav: clip.audio_path(corpus),
        duration_ms: clip.duration_ms,
        category: clip.category.clone(),
        language: clip.language.clone(),
    }
}

/// Which clips a model runs, and why it skips the others.
pub fn eligible(model: &BenchModel, clips: &[Clip], force_all: bool) -> (Vec<Clip>, Vec<(String, String)>) {
    let mut run = Vec::new();
    let mut skipped = Vec::new();
    for clip in clips {
        let reason = if model.max_duration_ms().is_some_and(|max| clip.duration_ms > max) {
            Some(format!("longer than the model's {} s limit", model.max_duration_ms().unwrap_or(0) / 1000))
        } else if clip.category == "short" && model.long_only() && !force_all {
            Some("file-transcription model: long clips only (use --force-all-clips)".to_string())
        } else {
            None
        };
        match reason {
            Some(reason) => skipped.push((clip.id.clone(), reason)),
            None => run.push(clip.clone()),
        }
    }
    (run, skipped)
}

fn run_id() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

fn mode_specs(paths: &Paths, names: &[String]) -> Result<(Vec<ModeSpec>, usize)> {
    let has_db = paths.db.is_file();
    let prompt = if has_db { vocabulary::build_asr_prompt_from_db_path(&paths.db)? } else { None };
    let term_count = if has_db { vocabulary::list_vocabulary_terms_from_db_path(&paths.db)?.len() } else { 0 };
    let mut modes = Vec::new();
    for name in names {
        modes.push(match name.as_str() {
            "raw" => ModeSpec { name: "raw".into(), prompt: None, prompt_terms: vec![], correct: false },
            "app" => ModeSpec {
                name: "app".into(),
                prompt: prompt.as_ref().map(|p| p.text.clone()),
                prompt_terms: prompt.as_ref().map(|p| p.terms.clone()).unwrap_or_default(),
                correct: has_db,
            },
            other => bail!("Unknown mode '{other}' (use raw, app or raw,app)"),
        });
    }
    Ok((modes, term_count))
}

fn select_clips(paths: &Paths, options: &RunOptions) -> Result<Vec<Clip>> {
    let mut clips = if options.synthetic {
        corpus::synthetic(paths)?
    } else {
        let manifest = corpus::Manifest::load(paths)?;
        if manifest.clips.is_empty() {
            bail!("The corpus is empty. Import recordings first (`npm run bench -- corpus import <folder>`), or try `--synthetic`.");
        }
        let unverified = manifest.clips.iter().filter(|c| !c.verified()).count();
        if unverified > 0 && !options.include_unverified {
            eprintln!("note: {unverified} unverified clip(s) left out (`corpus review` them, or pass --include-unverified)");
        }
        manifest
            .clips
            .into_iter()
            .filter(|clip| clip.verified() || options.include_unverified)
            .collect()
    };
    if let Some(category) = options.category.as_deref().filter(|c| *c != "all") {
        clips.retain(|clip| clip.category == category);
    }
    if let Some(languages) = &options.languages {
        let wanted: Vec<&str> = languages.split(',').map(str::trim).collect();
        clips.retain(|clip| wanted.contains(&clip.language.as_str()));
    }
    if let Some(filter) = &options.clip_filter {
        let wanted: Vec<&str> = filter.split(',').map(str::trim).collect();
        clips.retain(|clip| wanted.iter().any(|w| clip.id == *w || (w.ends_with('*') && clip.id.starts_with(w.trim_end_matches('*')))));
    }
    if options.quick {
        let mut per_language: HashMap<String, usize> = HashMap::new();
        clips.retain(|clip| {
            let count = per_language.entry(format!("{}-{}", clip.language, clip.category)).or_default();
            *count += 1;
            *count <= if clip.category == "short" { 3 } else { 1 }
        });
    }
    if clips.is_empty() {
        bail!("No clips match the filters.");
    }
    Ok(clips)
}

fn preflight(options: &RunOptions) -> Result<(system::Power, Option<String>)> {
    let power = system::power();
    if let Some(problem) = power.problem().filter(|_| !options.allow_battery) {
        bail!("Timings would be throttled: {problem}. Fix it, or pass --allow-battery to measure anyway.");
    }
    let running = system::running_app_processes();
    if !running.is_empty() && !options.allow_running_app {
        bail!(
            "Blabber is running ({}). Quit it so it does not compete for memory, the GPU or the Qwen lock, or pass --allow-running-app.",
            running.join("; ")
        );
    }
    let thermal = system::thermal();
    if thermal.as_deref().is_some_and(|t| t != "nominal") {
        eprintln!("warning: thermal state is {}", thermal.as_deref().unwrap_or("?"));
    }
    Ok((power, thermal))
}

pub fn run(paths: &Paths, options: &RunOptions) -> Result<PathBuf> {
    let (power, thermal) = preflight(options)?;
    let (run_dir, plan) = match &options.resume {
        Some(dir) => {
            let run_dir = RunDir { dir: dir.clone() };
            let plan = run_dir.load_plan()?;
            println!("Resuming {}", display(dir, &paths.repo));
            if let Some(redo) = options.redo.as_deref() {
                let ids: Vec<String> = models::select(plan.models.clone(), Some(redo))?.into_iter().map(|m| m.id).collect();
                discard_results(&run_dir, &ids)?;
                println!("  measuring again: {}", ids.join(", "));
            }
            (run_dir, plan)
        }
        None => {
            let settings = paths
                .db
                .is_file()
                .then(|| storage::get_settings_from_db_path(&paths.db).ok())
                .flatten();
            let chunk_ms = settings.as_ref().map(|s| s.live_pair_chunk_ms).unwrap_or(560);
            println!("Discovering models (verifies Qwen3-ASR files, takes a few seconds)…");
            let (available, unavailable) = models::discover(paths, chunk_ms)?;
            for missing in &unavailable {
                println!("  not runnable: {} — {}", missing.id, missing.reason);
            }
            let mut selected = models::select(available, options.models.as_deref())?;
            let clips = select_clips(paths, options)?;
            let (modes, vocabulary_term_count) = mode_specs(paths, &options.modes)?;
            let seed = options.seed.unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64);
            shuffle(&mut selected, seed);
            // Models that peak at many GB push others into swap; run them last.
            selected.sort_by_key(|model| model.long_only());
            let (git_commit, git_dirty) = system::git_state(&paths.repo);
            let id = run_id();
            let run_dir = RunDir { dir: paths.runs.join(&id) };
            std::fs::create_dir_all(&run_dir.dir)?;
            let plan = Plan {
                id,
                started_at: chrono::Local::now().to_rfc3339(),
                seed,
                settings: Settings {
                    repeats: options.repeats,
                    long_repeats: options.long_repeats,
                    stream_repeats: options.stream_repeats,
                    modes: options.modes.clone(),
                    language_mode: if options.fixed_language { "fixed" } else { "auto" }.into(),
                    synthetic: options.synthetic,
                    quick: options.quick,
                    cooldown_seconds: options.cooldown_seconds,
                    force_all_clips: options.force_all_clips,
                },
                machine: system::machine(),
                git_commit,
                git_dirty,
                power,
                thermal_at_start: thermal,
                corpus_path: if options.synthetic {
                    display(&paths.fixtures, &paths.repo)
                } else {
                    display(&paths.corpus, &paths.repo)
                },
                models: selected,
                clips,
                modes,
                vocabulary_term_count,
            };
            write_atomic(&run_dir.plan(), serde_json::to_string_pretty(&plan)?.as_bytes())?;
            (run_dir, plan)
        }
    };

    let done: HashSet<(String, String, String)> = run_dir
        .load_events()
        .into_iter()
        .filter_map(|line| match line.event {
            Event::Clip(output) => Some((line.model, output.clip, output.mode)),
            _ => None,
        })
        .collect();
    let corpus_dir = if plan.settings.synthetic { paths.fixtures.clone() } else { paths.corpus.clone() };
    let audio_seconds: u64 = plan.clips.iter().map(|c| c.duration_ms).sum::<u64>() / 1000;
    println!(
        "\nRun {}: {} model(s) × {} clip(s) ({} min of audio), modes {}",
        plan.id,
        plan.models.len(),
        plan.clips.len(),
        audio_seconds / 60,
        plan.settings.modes.join(",")
    );
    let mut log = OpenOptions::new().create(true).append(true).open(run_dir.log())?;
    let mut first = true;
    for (index, model) in plan.models.iter().enumerate() {
        let (clips, skipped) = eligible(model, &plan.clips, plan.settings.force_all_clips);
        let pending: Vec<Clip> = clips
            .into_iter()
            .filter(|clip| plan.modes.iter().any(|mode| !done.contains(&(model.id.clone(), clip.id.clone(), mode.name.clone()))))
            .collect();
        println!("\n[{}/{}] {} — {} clip(s){}", index + 1, plan.models.len(), model.id, pending.len(),
            if skipped.is_empty() { String::new() } else { format!(", {} skipped", skipped.len()) });
        if pending.is_empty() {
            if !skipped.is_empty() && options.resume.is_none() {
                append(&run_dir.models(), &ModelLine { model: model.id.clone(), skipped, ..Default::default() })?;
            }
            continue;
        }
        if !first {
            cool_down(plan.settings.cooldown_seconds);
        }
        if let Some(problem) = system::power().problem().filter(|_| !options.allow_battery) {
            println!(
                "\nStopping before {}: {problem}.\nEverything so far is saved. Continue with:\n  npm run bench -- run --resume {}",
                model.id,
                display(&run_dir.dir, &paths.repo)
            );
            bail!("run paused: {problem}");
        }
        first = false;
        let job = ChildJob {
            model: model.clone(),
            models_dir: paths.models.clone(),
            db_path: paths.db.clone(),
            temp_dir: paths.temp(),
            clips: pending.iter().map(|clip| child_clip(clip, &corpus_dir)).collect(),
            modes: plan.modes.clone(),
            repeats: plan.settings.repeats,
            long_repeats: plan.settings.long_repeats,
            stream_repeats: plan.settings.stream_repeats,
            fixed_language: plan.settings.language_mode == "fixed",
            draft: false,
            measure_load: true,
        };
        let clip_index: HashMap<String, &Clip> = pending.iter().map(|c| (c.id.clone(), c)).collect();
        let events_path = run_dir.events();
        let mut info = run_model(&job, &mut log, &mut |event| {
            let _ = append(&events_path, &EventLine { model: model.id.clone(), event: event.clone() });
            print_progress(event, &clip_index);
        })?;
        info.skipped = skipped;
        append(&run_dir.models(), &info)?;
    }

    println!("\nScoring…");
    let results = report::build(paths, &run_dir, true)?;
    let html = report::write(&run_dir.dir, &results)?;
    report::print_leaderboard(&results);
    if options.export_summary {
        let (json, html) = report::export_summary(paths, &results)?;
        println!("Summary (no transcripts): {} and {}", display(&json, &paths.repo), display(&html, &paths.repo));
    }
    println!("\nReport: {}", display(&html, &paths.repo));
    if options.open {
        let _ = Command::new("open").arg(&html).status();
    }
    Ok(html)
}

/// Moves a model's recorded results aside (kept as *.bak) so a resume runs it again.
fn discard_results(run_dir: &RunDir, ids: &[String]) -> Result<()> {
    for path in [run_dir.events(), run_dir.models()] {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        std::fs::write(path.with_extension("jsonl.bak"), &text)?;
        let kept: String = text
            .lines()
            .filter(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| value["model"].as_str().map(|m| !ids.iter().any(|id| id == m)))
                    .unwrap_or(true)
            })
            .map(|line| format!("{line}\n"))
            .collect();
        write_atomic(&path, kept.as_bytes())?;
    }
    Ok(())
}

fn cool_down(seconds: u64) {
    if seconds == 0 {
        return;
    }
    println!("  cooling down {seconds} s…");
    std::thread::sleep(Duration::from_secs(seconds));
    let began = Instant::now();
    while system::thermal().is_some_and(|t| t != "nominal") && began.elapsed() < Duration::from_secs(180) {
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn print_progress(event: &Event, clips: &HashMap<String, &Clip>) {
    match event {
        Event::Loaded { cold_ms, warm_ms, per_transcription } => {
            if *per_transcription {
                println!("  loads per transcription (worker process per file)");
            } else {
                println!(
                    "  loaded: first {} · second {}",
                    cold_ms.map(|v| format!("{:.1} s", v as f64 / 1000.0)).unwrap_or("—".into()),
                    warm_ms.map(|v| format!("{:.1} s", v as f64 / 1000.0)).unwrap_or("—".into())
                );
            }
        }
        Event::Clip(output) => {
            let wer = clips.get(&output.clip).and_then(|clip| {
                let langs = clip.langs();
                let score = match clip.scoring_segments() {
                    Some(segments) => asr_scoring::score(&asr_scoring::ReferenceText::Segments(&segments), &output.text, &langs),
                    None => asr_scoring::score(&asr_scoring::ReferenceText::Plain(&clip.reference), &output.text, &langs),
                };
                score.normalised.rate()
            });
            let time = output.runs_ms.first().or(output.compute_ms.as_ref()).copied().unwrap_or(0);
            println!(
                "  {:<22} {:<4} {:>7.2} s  WER {}",
                output.clip,
                output.mode,
                time as f64 / 1000.0,
                wer.map(|w| format!("{:5.1} %", w * 100.0)).unwrap_or_else(|| "    —".into())
            );
        }
        Event::ClipError { clip, mode, message } => {
            let first_line = message.lines().next().unwrap_or("");
            println!("  {:<22} {:<4} FAILED: {first_line}", if clip.is_empty() { "(engine)" } else { clip }, mode.as_deref().unwrap_or(""));
        }
        _ => {}
    }
}

enum Read {
    Event(Event),
    Closed,
}

/// Runs `job` in child processes until every clip finished, failed or timed
/// out. A crash or hang costs one clip; the next process continues after it.
pub fn run_model(job: &ChildJob, log: &mut File, on_event: &mut dyn FnMut(&Event)) -> Result<ModelLine> {
    let mut info = ModelLine { model: job.model.id.clone(), ..Default::default() };
    let mut remaining = job.clips.clone();
    let mut measure_load = job.measure_load;
    for _ in 0..MAX_SESSIONS_PER_MODEL {
        if remaining.is_empty() {
            break;
        }
        let mut session = job.clone();
        session.clips = remaining.clone();
        session.measure_load = measure_load;
        let durations: HashMap<String, u64> = remaining.iter().map(|c| (c.id.clone(), c.duration_ms)).collect();

        let executable = std::env::current_exe()?;
        let mut command = Command::new(executable);
        command
            .arg(CHILD_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log.try_clone()?));
        writeln!(log, "\n=== {} ({} clips) ===", job.model.id, remaining.len())?;
        let mut child = command.spawn().context("could not start the benchmark child process")?;
        let pid = child.id() as i32;
        {
            let mut stdin = child.stdin.take().context("child stdin")?;
            serde_json::to_writer(&mut stdin, &session)?;
            stdin.write_all(b"\n")?;
        }
        let stdout = child.stdout.take().context("child stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<Event>(&line) {
                    if tx.send(Read::Event(event)).is_err() {
                        return;
                    }
                }
            }
            let _ = tx.send(Read::Closed);
        });
        let sampler = Sampler::start(pid);
        let mut deadline = Instant::now() + LOAD_DEADLINE;
        let mut current: Option<String> = None;
        let mut finished: HashSet<String> = HashSet::new();
        let mut loaded = false;
        let mut clean_exit = false;
        let failure = loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait.max(Duration::from_millis(1))) {
                Ok(Read::Event(event)) => {
                    match &event {
                        Event::Loaded { cold_ms, warm_ms, per_transcription } => {
                            loaded = true;
                            if info.cold_ms.is_none() {
                                info.cold_ms = *cold_ms;
                                info.warm_ms = *warm_ms;
                            }
                            info.per_transcription = *per_transcription;
                        }
                        Event::Started { clip, runs } => {
                            let duration = durations.get(clip).copied().unwrap_or(60_000);
                            // Generous: slow models (VibeVoice, MOSS on CPU) take several × real time.
                            deadline = Instant::now()
                                + Duration::from_secs(180)
                                + Duration::from_millis(duration * 12 * (*runs as u64 + 1));
                            if let Some(previous) = current.replace(clip.clone()) {
                                finished.insert(previous);
                            }
                        }
                        Event::ClipError { clip, message, .. } if clip.is_empty() => {
                            info.error = Some(message.clone());
                        }
                        Event::Done => {
                            clean_exit = true;
                        }
                        _ => {}
                    }
                    if !matches!(event, Event::ClipError { ref clip, .. } if clip.is_empty()) {
                        on_event(&event);
                    }
                    if clean_exit {
                        break None;
                    }
                }
                Ok(Read::Closed) => break Some("the engine process ended unexpectedly"),
                Err(_) => break Some("timed out"),
            }
        };
        if failure.is_some() {
            system::kill_tree(pid);
        }
        let _ = child.wait();
        info.peak_memory_bytes = info.peak_memory_bytes.max(sampler.peak_bytes());
        info.wired_delta_bytes = match (info.wired_delta_bytes, sampler.wired_delta_bytes()) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        drop(sampler);
        if let Some(clip) = &current {
            finished.insert(clip.clone());
        }
        if let Some(reason) = failure {
            if let Some(clip) = &current {
                for mode in &job.modes {
                    on_event(&Event::ClipError { clip: clip.clone(), mode: Some(mode.name.clone()), message: format!("{reason} while transcribing this clip") });
                }
            } else {
                info.error.get_or_insert_with(|| format!("{reason} while loading the model"));
            }
        }
        if clean_exit || !loaded {
            if !loaded && info.error.is_none() {
                info.error = Some("the model failed to load".into());
            }
            if let Some(error) = &info.error {
                if !loaded {
                    on_event(&Event::ClipError { clip: String::new(), mode: None, message: error.clone() });
                    println!("  model failed: {}", error.lines().next().unwrap_or(""));
                }
            }
            break;
        }
        remaining.retain(|clip| !finished.contains(&clip.id));
        measure_load = false;
    }
    Ok(info)
}
