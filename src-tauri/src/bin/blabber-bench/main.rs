//! `blabber-bench`: compares every installed speech-to-text engine on the same
//! recordings, for speed and word error rate. See docs/asr-benchmark-plan.md.
//!
//! npm run bench -- <command>   (builds with `--release --features bench`)
mod corpus;
mod draft;
mod engine;
mod models;
mod paths;
mod report;
mod review_server;
mod runner;
mod system;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use speech_to_text_lib::{storage, vocabulary};

use crate::paths::{display, Paths};

const USAGE: &str = "\
blabber-bench — speed and word error rate of Blabber's speech-to-text models

USAGE
  npm run bench -- <command> [options]

COMMANDS
  doctor                         Machine, power, models, helpers and corpus status
  corpus import <files|dirs>     Add recordings (any format). Files named after a script id
                                 (de-s05.m4a, de-s05@noisy.m4a) take its reference automatically
      --lang de|en|fr|mixed|none   Language for files that are not scripts
      --tags a,b  --speaker NAME  --force (replace clips with the same id)
  corpus draft                   Draft references with two engines (--models a,b  --redo)
  corpus review                  Open the review page to listen, correct and verify
      --port N  --no-open
  corpus check                   Validate the corpus manifest and audio
  run                            Run the benchmark and write the report
      --models a,b*              Model ids or globs (default: all runnable)
      --clips short|long|all     --lang de,en,fr,mixed,none   --only id,prefix*
      --mode app|raw|raw,app     app = with your vocabulary (default), raw = engine only
      --repeats N (3)  --long-repeats N (1)  --stream-repeats N (1)
      --fixed-language           Tell engines the clip language instead of auto-detect
      --quick                    1 repeat, 3 short + 1 long clip per language
      --synthetic                Use the macOS `say` fixtures (no corpus needed)
      --include-unverified       Also score clips whose reference is not verified
      --force-all-clips          Run MOSS and VibeVoice on short clips too
      --cooldown S (20)  --seed N  --allow-battery  --allow-running-app
      --resume RUN_DIR           Continue an interrupted run
      --rerun a,b*               With --resume: measure these models again (e.g. after interference)
      --export-summary           Also write a transcript-free summary to benchmark-results/
      --open                     Open the report when done
  report <run-dir>               Re-score a run and rewrite its report (--export-summary --open)
  compare <run-a> <run-b>        Before/after table of two runs

GLOBAL OPTIONS
  --models-dir DIR  --corpus DIR  --runs-dir DIR
";

const BOOLEAN_FLAGS: &[&str] = &[
    "force", "redo", "no-open", "fixed-language", "quick", "synthetic", "include-unverified",
    "force-all-clips", "allow-battery", "allow-running-app", "export-summary", "open", "help",
];

struct Args {
    positional: Vec<String>,
    flags: HashMap<String, String>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let mut positional = Vec::new();
        let mut flags = HashMap::new();
        let mut iter = raw.into_iter();
        while let Some(arg) = iter.next() {
            if let Some(name) = arg.strip_prefix("--") {
                let (name, inline) = match name.split_once('=') {
                    Some((name, value)) => (name.to_string(), Some(value.to_string())),
                    None => (name.to_string(), None),
                };
                if BOOLEAN_FLAGS.contains(&name.as_str()) {
                    flags.insert(name, "true".into());
                } else {
                    let value = inline.or_else(|| iter.next()).with_context(|| format!("--{name} needs a value"))?;
                    flags.insert(name, value);
                }
            } else if arg == "-h" {
                flags.insert("help".into(), "true".into());
            } else {
                positional.push(arg);
            }
        }
        Ok(Self { positional, flags })
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    fn value(&self, name: &str) -> Option<String> {
        self.flags.get(name).cloned()
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T> {
        match self.flags.get(name) {
            Some(value) => value.parse().ok().with_context(|| format!("--{name} expects a number")),
            None => Ok(default),
        }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.flags.get(name).map(PathBuf::from)
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some(runner::CHILD_ARG) {
        if let Err(error) = engine::run_child() {
            eprintln!("child: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run(raw) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(raw: Vec<String>) -> Result<()> {
    let args = Args::parse(raw)?;
    if args.flag("help") || args.positional.is_empty() {
        print!("{USAGE}");
        return Ok(());
    }
    let paths = Paths::new(args.path("models-dir"), args.path("corpus"), args.path("runs-dir"))?;
    models::export_worker_env(&paths);
    let command: Vec<&str> = args.positional.iter().map(String::as_str).collect();
    match command.as_slice() {
        ["doctor"] => doctor(&paths),
        ["corpus", "import", inputs @ ..] => {
            if inputs.is_empty() {
                bail!("corpus import needs files or folders");
            }
            let inputs: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            corpus::import(
                &paths,
                &inputs,
                &corpus::ImportOptions {
                    language: args.value("lang"),
                    tags: args
                        .value("tags")
                        .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                        .unwrap_or_default(),
                    speaker: args.value("speaker"),
                    force: args.flag("force"),
                },
            )
        }
        ["corpus", "draft"] => draft::draft(&paths, args.value("models").as_deref(), args.flag("redo")),
        ["corpus", "review"] => review_server::serve(&paths, args.number("port", 8765)?, !args.flag("no-open")),
        ["corpus", "check"] => {
            if !corpus::check(&paths)? {
                std::process::exit(2);
            }
            Ok(())
        }
        ["run"] => {
            let modes: Vec<String> = args
                .value("mode")
                .unwrap_or_else(|| "app".into())
                .split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();
            let quick = args.flag("quick");
            let options = runner::RunOptions {
                models: args.value("models"),
                category: args.value("clips"),
                languages: args.value("lang"),
                clip_filter: args.value("only"),
                modes,
                repeats: if quick { 1 } else { args.number("repeats", 3)? },
                long_repeats: if quick { 1 } else { args.number("long-repeats", 1)? },
                stream_repeats: if quick { 1 } else { args.number("stream-repeats", 1)? },
                fixed_language: args.flag("fixed-language"),
                quick,
                synthetic: args.flag("synthetic"),
                include_unverified: args.flag("include-unverified"),
                force_all_clips: args.flag("force-all-clips"),
                cooldown_seconds: args.number("cooldown", if quick { 5 } else { 20 })?,
                allow_battery: args.flag("allow-battery"),
                allow_running_app: args.flag("allow-running-app"),
                seed: args.value("seed").map(|s| s.parse()).transpose().context("--seed expects a number")?,
                resume: args.path("resume"),
                redo: args.value("rerun"),
                export_summary: args.flag("export-summary"),
                open: args.flag("open"),
            };
            runner::run(&paths, &options).map(|_| ())
        }
        ["report", dir] => {
            let run_dir = runner::RunDir { dir: PathBuf::from(dir) };
            let results = report::build(&paths, &run_dir, true)?;
            let html = report::write(&run_dir.dir, &results)?;
            report::print_leaderboard(&results);
            if args.flag("export-summary") {
                let (json, page) = report::export_summary(&paths, &results)?;
                println!("Summary: {} and {}", display(&json, &paths.repo), display(&page, &paths.repo));
            }
            println!("\nReport: {}", display(&html, &paths.repo));
            if args.flag("open") {
                let _ = std::process::Command::new("open").arg(&html).status();
            }
            Ok(())
        }
        ["compare", a, b] => {
            report::compare(&report::load(&PathBuf::from(a))?, &report::load(&PathBuf::from(b))?);
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            bail!("unknown command: {}", args.positional.join(" "))
        }
    }
}

fn doctor(paths: &Paths) -> Result<()> {
    let machine = system::machine();
    let power = system::power();
    let (commit, dirty) = system::git_state(&paths.repo);
    println!("Machine   {} · {:.0} GB · {} cores · {}", machine.cpu, machine.memory_bytes as f64 / 1_073_741_824.0, machine.cores, machine.os);
    println!(
        "Power     {}{}{}{}",
        power.source.as_deref().unwrap_or("unknown"),
        power.adapter_watts.map(|w| format!(" · {w} W adapter")).unwrap_or_default(),
        power.battery_percent.map(|p| format!(" · battery {p} %")).unwrap_or_default(),
        if power.low_power_mode == Some(true) { " · Low Power Mode ON" } else { "" }
    );
    println!("Thermal   {}", system::thermal().unwrap_or_else(|| "unknown".into()));
    println!(
        "Code      {}{}",
        commit.as_deref().unwrap_or("unknown commit"),
        if dirty == Some(true) { " (uncommitted changes)" } else { "" }
    );
    let mut warnings = Vec::new();
    if let Some(problem) = power.problem() {
        warnings.push(format!("Timings would be throttled: {problem}."));
    }
    let running = system::running_app_processes();
    if !running.is_empty() {
        warnings.push(format!("Blabber is running ({}); quit it before a run.", running.join("; ")));
    }

    println!("\nModels    {}", paths.models.display());
    let settings = storage::get_settings_from_db_path(&paths.db).ok();
    let chunk_ms = settings.as_ref().map(|s| s.live_pair_chunk_ms).unwrap_or(560);
    let (available, unavailable) = models::discover(paths, chunk_ms)?;
    for model in &available {
        let size = model.size_bytes.map(|b| format!("{:.2} GB", b as f64 / 1e9)).unwrap_or_default();
        let note = if model.long_only() { "long clips only by default" } else if model.streaming() { "streaming" } else { "" };
        println!("  ✓ {:<20} {:<22} {:>9}  {}", model.id, model.engine_name, size, note);
    }
    for missing in &unavailable {
        println!("  ✗ {:<20} {}", missing.id, missing.reason);
    }

    let terms = vocabulary::list_vocabulary_terms_from_db_path(&paths.db).map(|t| t.len()).ok();
    println!(
        "\nVocabulary {}",
        match terms {
            Some(count) => format!("{count} term(s) from the app database (used in app mode)"),
            None => "app database not found: app mode equals raw mode".into(),
        }
    );
    if let Some(settings) = &settings {
        println!(
            "App       shortcut model {} · language {:?}",
            settings.shortcut_dictation_selected_model_id.as_deref().unwrap_or("default"),
            settings.language_mode
        );
    }

    let scripts = corpus::Scripts::load()?;
    let manifest = corpus::Manifest::load(paths)?;
    let verified = manifest.clips.iter().filter(|c| c.verified()).count();
    println!("\nCorpus    {}", display(&paths.corpus, &paths.repo));
    println!("  {} clip(s), {} verified · {} read-aloud scripts available", manifest.clips.len(), verified, scripts.len());
    println!(
        "  synthetic fixtures: {}",
        if paths.fixtures.is_dir() { "present (run --synthetic)" } else { "missing (python3 workers/r2t2/make_probe_audio.py --individual)" }
    );
    println!("Runs      {}", display(&paths.runs, &paths.repo));

    if !warnings.is_empty() {
        println!();
        for warning in warnings {
            println!("warning: {warning}");
        }
    }
    println!("\nNext: {}", if manifest.clips.is_empty() {
        "record the scripts in docs/asr-benchmark-scripts.md, then `npm run bench -- corpus import <folder>`"
    } else if verified < manifest.clips.len() {
        "`npm run bench -- corpus draft`, then `npm run bench -- corpus review`"
    } else {
        "`npm run bench -- run --open`"
    });
    Ok(())
}
