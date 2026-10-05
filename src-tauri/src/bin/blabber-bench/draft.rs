//! Draft references: two different engines transcribe every unverified clip,
//! so the review page can show where they disagree.
use std::collections::HashMap;
use std::fs::OpenOptions;

use anyhow::{bail, Result};
use speech_to_text_lib::storage;

use crate::corpus::{self, Draft, DraftSegment, Manifest};
use crate::engine::{ChildJob, Event, ModeSpec};
use crate::models::{self, BenchModel};
use crate::paths::Paths;
use crate::runner;

const FIRST_CHOICE: &[&str] = &["whisper-large-v3", "whisper-turbo", "whisper-medium", "whisper-turbo-q5"];
const SECOND_CHOICE: &[&str] = &["qwen3-asr", "whisper-turbo-q5", "whisper-medium", "whisper-small"];

fn pick(models: &[BenchModel], filter: Option<&str>) -> Result<Vec<BenchModel>> {
    let batch: Vec<BenchModel> = models.iter().filter(|m| !m.streaming() && !m.long_only()).cloned().collect();
    if let Some(filter) = filter {
        return models::select(batch, Some(filter));
    }
    let find = |ids: &[&str], not: Option<&str>| {
        ids.iter()
            .filter(|id| Some(**id) != not)
            .find_map(|id| batch.iter().find(|m| m.id == *id))
            .cloned()
    };
    let first = find(FIRST_CHOICE, None);
    let second = find(SECOND_CHOICE, first.as_ref().map(|m| m.id.as_str()));
    let picked: Vec<BenchModel> = first.into_iter().chain(second).collect();
    if picked.is_empty() {
        bail!("No Whisper or Qwen model is installed to draft references with.");
    }
    Ok(picked)
}

pub fn draft(paths: &Paths, filter: Option<&str>, redo: bool) -> Result<()> {
    let mut manifest = Manifest::load(paths)?;
    let targets: Vec<corpus::Clip> = manifest
        .clips
        .iter()
        .filter(|clip| clip.language != "none" && !clip.verified())
        .filter(|clip| redo || corpus::load_drafts(paths, &clip.id).is_empty())
        .cloned()
        .collect();
    if targets.is_empty() {
        println!("Every unverified clip already has drafts (use --redo to draft again).");
        return Ok(());
    }
    if !runner_app_free() {
        eprintln!("warning: Blabber is running; quit it if drafting with Qwen3-ASR fails on its lock.");
    }
    let chunk_ms = storage::get_settings_from_db_path(&paths.db).map(|s| s.live_pair_chunk_ms).unwrap_or(560);
    println!("Discovering models…");
    let (available, _) = models::discover(paths, chunk_ms)?;
    let drafters = pick(&available, filter)?;
    println!(
        "Drafting {} clip(s) with {}",
        targets.len(),
        drafters.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(" and ")
    );
    std::fs::create_dir_all(&paths.runs)?;
    let mut log = OpenOptions::new().create(true).append(true).open(paths.runs.join("draft.log"))?;
    let mut drafts: HashMap<String, Vec<Draft>> = HashMap::new();
    for model in &drafters {
        let (clips, _) = runner::eligible(model, &targets, true);
        println!("\n{} — {} clip(s)", model.id, clips.len());
        let job = ChildJob {
            model: model.clone(),
            models_dir: paths.models.clone(),
            db_path: paths.db.clone(),
            temp_dir: paths.temp(),
            clips: clips.iter().map(|clip| runner::child_clip(clip, &paths.corpus)).collect(),
            modes: vec![ModeSpec { name: "raw".into(), prompt: None, prompt_terms: vec![], correct: false }],
            repeats: 1,
            long_repeats: 1,
            stream_repeats: 1,
            fixed_language: false,
            draft: true,
            measure_load: false,
        };
        runner::run_model(&job, &mut log, &mut |event| match event {
            Event::Clip(output) => {
                println!("  {:<22} {} words", output.clip, output.text.split_whitespace().count());
                drafts.entry(output.clip.clone()).or_default().push(Draft {
                    model: model.id.clone(),
                    text: output.text.clone(),
                    segments: output
                        .segments
                        .iter()
                        .map(|s| DraftSegment { start_ms: s.start_ms, end_ms: s.end_ms, text: s.text.clone() })
                        .collect(),
                });
            }
            Event::ClipError { clip, message, .. } => {
                println!("  {:<22} FAILED: {}", clip, message.lines().next().unwrap_or(""))
            }
            _ => {}
        })?;
    }
    let mut updated = 0;
    for (id, new_drafts) in drafts {
        let mut all = corpus::load_drafts(paths, &id);
        all.retain(|existing| !new_drafts.iter().any(|d| d.model == existing.model));
        all.extend(new_drafts);
        all.sort_by_key(|d| drafters.iter().position(|m| m.id == d.model).unwrap_or(usize::MAX));
        corpus::save_drafts(paths, &id, &all)?;
        if let Some(clip) = manifest.get_mut(&id) {
            if clip.script.is_none() && clip.reference.trim().is_empty() {
                if let Some(first) = all.first() {
                    clip.reference = first.text.clone();
                }
            }
            if clip.script.is_none() {
                clip.reference_drafted_by = all.iter().map(|d| d.model.clone()).collect();
            }
        }
        updated += 1;
    }
    manifest.save(paths)?;
    println!("\nDrafted {updated} clip(s). Next: `npm run bench -- corpus review` to listen and verify.");
    Ok(())
}

fn runner_app_free() -> bool {
    crate::system::running_app_processes().is_empty()
}
