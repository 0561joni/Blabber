//! Scores a run and writes `results.json`, `report.html` and the shareable
//! summary (see `results-schema.md`).
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use speech_to_text_lib::asr_scoring::{self, ClipErrors, ErrorCounts, Lang, ReferenceText};

use crate::corpus::Clip;
use crate::engine::{ClipOutput, Event};
use crate::paths::{relative, write_atomic, Paths};
use crate::runner::{ModelLine, Plan, RunDir};

const TEMPLATE: &str = include_str!("report_template.html");
const PLACEHOLDER: &str = "/*__BENCH_DATA__*/null";
const BOOTSTRAP_ITERATIONS: usize = 2_000;
const STREAM_SUFFIX: &str = "-stream";

/// A scored row with the facts aggregation needs.
struct Scored {
    model: String,
    clip: String,
    mode: String,
    score: Option<asr_scoring::PairScore>,
    terms_found: usize,
    terms_total: usize,
    text_on_silence: bool,
    language_correct: Option<bool>,
    runs_ms: Vec<u64>,
    first_word_ms: Option<u64>,
    rtf: Option<f64>,
    failed: bool,
}

impl Scored {
    fn hallucinated(&self) -> bool {
        self.text_on_silence
            || self.score.as_ref().is_some_and(|s| s.repetition_loop || s.insertion_burst >= 5)
    }
}

fn median_u64(values: &[u64]) -> Option<u64> {
    asr_scoring::percentile(&values.iter().map(|&v| v as f64).collect::<Vec<_>>(), 50.0).map(|v| v.round() as u64)
}

fn counts_json(counts: &ErrorCounts) -> Value {
    json!({
        "referenceWords": counts.reference_words,
        "substitutions": counts.substitutions,
        "deletions": counts.deletions,
        "insertions": counts.insertions,
        "rate": counts.rate(),
    })
}

fn language_correct(clip: &Clip, detected: Option<&str>) -> Option<bool> {
    if clip.language == "none" {
        return None;
    }
    let detected = Lang::from_code(detected?)?;
    Some(clip.langs().contains(&detected))
}

fn reference_tokens(clip: &Clip) -> Vec<String> {
    match clip.scoring_segments() {
        Some(segments) => asr_scoring::normalise_segments(&segments),
        None => asr_scoring::normalise(&clip.reference, &clip.langs()),
    }
}

fn score_text(clip: &Clip, text: &str) -> asr_scoring::PairScore {
    let langs = clip.langs();
    match clip.scoring_segments() {
        Some(segments) => asr_scoring::score(&ReferenceText::Segments(&segments), text, &langs),
        None => asr_scoring::score(&ReferenceText::Plain(&clip.reference), text, &langs),
    }
}

/// The last outcome per (model, clip, mode): a later success replaces an
/// earlier failure (resumed runs retry failures).
fn latest(events: Vec<crate::runner::EventLine>) -> (BTreeMap<(String, String, String), Result<ClipOutput, String>>, HashMap<String, String>) {
    let mut outcomes = BTreeMap::new();
    let mut model_errors = HashMap::new();
    for line in events {
        match line.event {
            Event::Clip(output) => {
                outcomes.insert((line.model, output.clip.clone(), output.mode.clone()), Ok(output));
            }
            Event::ClipError { clip, message, .. } if clip.is_empty() => {
                model_errors.insert(line.model, message);
            }
            Event::ClipError { clip, mode, message } => {
                let key = (line.model, clip, mode.unwrap_or_default());
                if !matches!(outcomes.get(&key), Some(Ok(_))) {
                    outcomes.insert(key, Err(message));
                }
            }
            _ => {}
        }
    }
    (outcomes, model_errors)
}

pub fn build(paths: &Paths, run_dir: &RunDir, include_text: bool) -> Result<Value> {
    let mut plan: Plan = run_dir.load_plan()?;
    if !plan.settings.synthetic {
        refresh_references(paths, &mut plan);
    }
    let (outcomes, model_errors) = latest(run_dir.load_events());
    let model_lines = run_dir.load_models();
    let clips: HashMap<&str, &Clip> = plan.clips.iter().map(|c| (c.id.as_str(), c)).collect();
    let corpus_dir = if plan.settings.synthetic { paths.fixtures.clone() } else { paths.corpus.clone() };

    // Rows, including the live pair's preview text as a derived model.
    let mut rows = Vec::new();
    let mut scored = Vec::new();
    let mut derived: BTreeSet<String> = BTreeSet::new();
    for ((model, clip_id, mode), outcome) in &outcomes {
        let Some(clip) = clips.get(clip_id.as_str()) else { continue };
        let mut variants = vec![(model.clone(), outcome.clone().map(|o| (o.text.clone(), o)))];
        if let Ok(output) = outcome {
            if let Some(stream) = &output.stream_text {
                let id = format!("{model}{STREAM_SUFFIX}");
                derived.insert(model.clone());
                variants.push((id, Ok((stream.clone(), ClipOutput { runs_ms: vec![], early_ms: None, ..output.clone() }))));
            }
        }
        for (model_id, variant) in variants {
            let (row, facts) = row_for(clip, &model_id, mode, variant, include_text);
            rows.push(row);
            scored.push(facts);
        }
    }

    let clip_values: Vec<Value> = plan
        .clips
        .iter()
        .map(|clip| {
            let mut value = json!({
                "id": clip.id,
                "language": clip.language,
                "languages": clip.languages,
                "category": clip.category,
                "tags": clip.tags,
                "condition": clip.condition,
                "script": clip.script,
                "durationMs": clip.duration_ms,
                "referenceWords": reference_tokens(clip).len(),
                "referenceDraftedBy": clip.reference_drafted_by,
            });
            if include_text {
                value["reference"] = json!(clip.reference);
                value["terms"] = json!(clip.terms);
                if let Some(segments) = &clip.segments {
                    value["segments"] = json!(segments);
                }
                value["audioHref"] = json!(relative(&run_dir.dir, &clip.audio_path(&corpus_dir)).to_string_lossy().replace('\\', "/"));
            }
            value
        })
        .collect();

    let mut model_values = Vec::new();
    let mut model_order = Vec::new();
    for model in &plan.models {
        let lines: Vec<&ModelLine> = model_lines.iter().filter(|l| l.model == model.id).collect();
        let first = |f: fn(&ModelLine) -> Option<u64>| lines.iter().find_map(|l| f(l));
        let errors: Vec<Value> = outcomes
            .iter()
            .filter(|((m, _, _), o)| m == &model.id && o.is_err())
            .map(|((_, clip, mode), o)| json!({"clip": clip, "message": format!("{mode}: {}", o.as_ref().err().map(String::as_str).unwrap_or(""))}))
            .chain(model_errors.get(&model.id).map(|message| json!({"clip": null, "message": message})))
            .collect();
        let skipped: Vec<Value> = lines
            .iter()
            .flat_map(|l| l.skipped.iter())
            .map(|(clip, reason)| json!({"clip": clip, "reason": reason}))
            .collect();
        let peak = lines.iter().map(|l| l.peak_memory_bytes).max().filter(|&p| p > 0);
        let wired = lines.iter().filter_map(|l| l.wired_delta_bytes).max();
        let base = json!({
            "id": model.id,
            "appModelId": model.app_model_id,
            "name": model.name,
            "engine": model.engine_name,
            "kind": if model.streaming() { "streaming" } else { "batch" },
            "derivedFrom": null,
            "sizeBytes": model.size_bytes,
            "load": {
                "coldMs": first(|l| l.cold_ms),
                "warmMs": first(|l| l.warm_ms),
                "coremlCompileMs": null,
                "perTranscription": lines.iter().any(|l| l.per_transcription),
            },
            "peakMemoryBytes": peak,
            "wiredDeltaBytes": wired,
            "errors": errors,
            "skippedClips": skipped,
        });
        model_order.push(model.id.clone());
        model_values.push(base.clone());
        if derived.contains(&model.id) {
            let mut stream = base;
            let id = format!("{}{STREAM_SUFFIX}", model.id);
            stream["id"] = json!(id);
            stream["name"] = json!(format!("{} (live preview)", model.name));
            stream["derivedFrom"] = json!(model.id);
            stream["errors"] = json!([]);
            model_order.push(id);
            model_values.push(stream);
        }
    }

    let aggregates = aggregates(&plan, &clips, &scored, &model_order);
    let modes: Vec<String> = plan.settings.modes.clone();
    Ok(json!({
        "schemaVersion": 1,
        "tool": "blabber-bench",
        "textIncluded": include_text,
        "run": {
            "id": plan.id,
            "startedAt": plan.started_at,
            "finishedAt": chrono::Local::now().to_rfc3339(),
            "machine": plan.machine,
            "git": {"commit": plan.git_commit, "dirty": plan.git_dirty},
            "power": plan.power,
            "thermalAtStart": plan.thermal_at_start,
            "seed": plan.seed,
            "normalisationVersion": asr_scoring::NORMALISATION_VERSION,
            "settings": {
                "repeats": plan.settings.repeats,
                "longRepeats": plan.settings.long_repeats,
                "streamRepeats": plan.settings.stream_repeats,
                "modes": modes,
                "languageMode": plan.settings.language_mode,
                "synthetic": plan.settings.synthetic,
                "quick": plan.settings.quick,
                "cooldownSeconds": plan.settings.cooldown_seconds,
            },
            "vocabularyTermCount": plan.vocabulary_term_count,
        },
        "corpus": {"path": plan.corpus_path, "clipCount": plan.clips.len()},
        "clips": clip_values,
        "models": model_values,
        "results": rows,
        "aggregates": aggregates,
    }))
}

/// References verified after the run was recorded replace the snapshot in
/// plan.json (same clip id and audio), so re-scoring needs no re-run.
fn refresh_references(paths: &Paths, plan: &mut Plan) {
    let Ok(manifest) = crate::corpus::Manifest::load(paths) else { return };
    let mut updated = 0;
    for clip in &mut plan.clips {
        let Some(current) = manifest.clips.iter().find(|c| c.id == clip.id && c.audio_sha256 == clip.audio_sha256) else {
            continue;
        };
        if current.reference != clip.reference || current.reference_status != clip.reference_status || current.terms != clip.terms {
            clip.reference = current.reference.clone();
            clip.reference_status = current.reference_status.clone();
            clip.segments = current.segments.clone();
            clip.terms = current.terms.clone();
            clip.reference_drafted_by = current.reference_drafted_by.clone();
            updated += 1;
        }
    }
    if updated > 0 {
        eprintln!("note: {updated} reference(s) updated from the corpus since the run");
    }
}

fn row_for(clip: &Clip, model: &str, mode: &str, outcome: Result<(String, ClipOutput), String>, include_text: bool) -> (Value, Scored) {
    let mut facts = Scored {
        model: model.into(),
        clip: clip.id.clone(),
        mode: mode.into(),
        score: None,
        terms_found: 0,
        terms_total: 0,
        text_on_silence: false,
        language_correct: None,
        runs_ms: vec![],
        first_word_ms: None,
        rtf: None,
        failed: true,
    };
    let (text, output) = match outcome {
        Ok(value) => value,
        Err(message) => {
            let row = json!({
                "model": model, "clip": clip.id, "mode": mode, "detectedLanguage": null,
                "languageCorrect": null,
                "timings": {"runsMs": [], "releaseToTextMs": null, "releaseToTextEarlyMs": null, "computeMs": null, "rtf": null, "firstWordMs": null},
                "scores": null, "error": message,
            });
            return (row, facts);
        }
    };
    let score = score_text(clip, &text);
    let terms_found = clip.terms.iter().filter(|term| asr_scoring::term_found(term, &text)).count();
    let text_on_silence = score.reference_tokens.is_empty() && !score.hypothesis_tokens.is_empty();
    let language_ok = language_correct(clip, output.detected_language.as_deref());
    let short = clip.category == "short";
    let release = if short { median_u64(&output.runs_ms) } else { None };
    let first_word = median_u64(&output.first_word_ms);
    let rtf = output.compute_ms.filter(|_| clip.duration_ms > 0).map(|ms| ms as f64 / clip.duration_ms as f64);
    let mut row = json!({
        "model": model,
        "clip": clip.id,
        "mode": mode,
        "detectedLanguage": output.detected_language,
        "languageCorrect": language_ok,
        "timings": {
            "runsMs": output.runs_ms,
            "releaseToTextMs": release,
            "releaseToTextEarlyMs": output.early_ms,
            "computeMs": output.compute_ms,
            "rtf": rtf,
            "firstWordMs": first_word,
        },
        "scores": {
            "normalised": counts_json(&score.normalised),
            "formatted": counts_json(&score.formatted),
            "characters": counts_json(&score.characters),
            "termsFound": terms_found,
            "termsTotal": clip.terms.len(),
            "numbersCorrect": score.numbers_correct,
            "numbersTotal": score.numbers_total,
            "insertionBurst": score.insertion_burst,
            "repetitionLoop": score.repetition_loop,
            "textOnSilence": text_on_silence,
        },
        "error": null,
    });
    if include_text {
        row["text"] = json!(text);
        row["alignment"] = serde_json::to_value(&score.alignment).unwrap_or(Value::Null);
    }
    facts.terms_found = terms_found;
    facts.terms_total = clip.terms.len();
    facts.text_on_silence = text_on_silence;
    facts.language_correct = language_ok;
    facts.runs_ms = if short { output.runs_ms.clone() } else { vec![] };
    facts.first_word_ms = first_word;
    facts.rtf = rtf;
    facts.failed = false;
    facts.score = Some(score);
    (row, facts)
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn group(rows: &[&Scored], seed: u64) -> Value {
    let scored: Vec<&asr_scoring::PairScore> = rows
        .iter()
        .filter_map(|r| r.score.as_ref())
        .filter(|s| s.normalised.reference_words > 0)
        .collect();
    let mut normalised = ErrorCounts::default();
    let mut formatted = ErrorCounts::default();
    let mut characters = ErrorCounts::default();
    for score in &scored {
        normalised.add(&score.normalised);
        formatted.add(&score.formatted);
        characters.add(&score.characters);
    }
    let clip_errors: Vec<ClipErrors> = scored
        .iter()
        .map(|s| ClipErrors { errors: s.normalised.errors(), reference_words: s.normalised.reference_words })
        .collect();
    let (terms_found, terms_total) = rows.iter().fold((0, 0), |(f, t), r| (f + r.terms_found, t + r.terms_total));
    let (numbers_correct, numbers_total) = rows
        .iter()
        .filter_map(|r| r.score.as_ref())
        .fold((0, 0), |(c, t), s| (c + s.numbers_correct, t + s.numbers_total));
    let languages: Vec<bool> = rows.iter().filter_map(|r| r.language_correct).collect();
    let release: Vec<f64> = rows.iter().flat_map(|r| r.runs_ms.iter().map(|&v| v as f64)).collect();
    let first_word: Vec<f64> = rows.iter().filter_map(|r| r.first_word_ms.map(|v| v as f64)).collect();
    let rtf: Vec<f64> = rows.iter().filter_map(|r| r.rtf).collect();
    json!({
        "clips": rows.len(),
        "referenceWords": normalised.reference_words,
        "substitutions": normalised.substitutions,
        "deletions": normalised.deletions,
        "insertions": normalised.insertions,
        "werPooled": asr_scoring::pooled_rate(&clip_errors),
        "werMacro": asr_scoring::macro_rate(&clip_errors),
        "ci95": asr_scoring::bootstrap_ci(&clip_errors, BOOTSTRAP_ITERATIONS, seed).map(|(a, b)| [a, b]),
        "formattedPooled": formatted.rate(),
        "cerPooled": characters.rate(),
        "termRecall": ratio(terms_found, terms_total),
        "numberAccuracy": ratio(numbers_correct, numbers_total),
        "hallucinations": rows.iter().filter(|r| r.hallucinated()).count(),
        "languageAccuracy": ratio(languages.iter().filter(|&&ok| ok).count(), languages.len()),
        "releaseToTextP50": asr_scoring::percentile(&release, 50.0),
        "releaseToTextP95": asr_scoring::percentile(&release, 95.0),
        "firstWordP50": asr_scoring::percentile(&first_word, 50.0),
        "firstWordP95": asr_scoring::percentile(&first_word, 95.0),
        "rtfMedian": asr_scoring::percentile(&rtf, 50.0),
        "failures": rows.iter().filter(|r| r.failed).count(),
    })
}

fn grouped<'a>(rows: &[&'a Scored], key: impl Fn(&Scored) -> Vec<String>, seed: u64) -> Value {
    let mut groups: BTreeMap<String, Vec<&'a Scored>> = BTreeMap::new();
    for row in rows {
        for name in key(row) {
            groups.entry(name).or_default().push(row);
        }
    }
    Value::Object(groups.into_iter().map(|(name, rows)| (name, group(&rows, seed))).collect())
}

fn base_clip(clip: &Clip) -> String {
    clip.id.split('@').next().unwrap_or(&clip.id).to_string()
}

fn aggregates(plan: &Plan, clips: &HashMap<&str, &Clip>, scored: &[Scored], models: &[String]) -> Value {
    let seed = plan.seed;
    let clip_of = |row: &Scored| clips.get(row.clip.as_str()).copied();
    let mut by_model = Map::new();
    for model in models {
        let mut by_mode = Map::new();
        for mode in &plan.settings.modes {
            let rows: Vec<&Scored> = scored.iter().filter(|r| &r.model == model && &r.mode == mode).collect();
            if rows.is_empty() {
                continue;
            }
            by_mode.insert(mode.clone(), json!({
                "overall": group(&rows, seed),
                "byLanguage": grouped(&rows, |r| clip_of(r).map(|c| vec![c.language.clone()]).unwrap_or_default(), seed),
                "byCategory": grouped(&rows, |r| clip_of(r).map(|c| vec![c.category.clone()]).unwrap_or_default(), seed),
                "byTag": grouped(&rows, |r| {
                    let Some(clip) = clip_of(r) else { return vec![] };
                    let mut keys = clip.tags.clone();
                    keys.push(match &clip.condition {
                        Some(condition) => format!("condition:{condition}"),
                        None => "clean".into(),
                    });
                    keys
                }, seed),
            }));
        }
        by_model.insert(model.clone(), Value::Object(by_mode));
    }

    // Paired comparisons on clips both models scored (speech clips only).
    let mut pairwise = Map::new();
    for mode in &plan.settings.modes {
        let mut errors: HashMap<(&str, &str), ClipErrors> = HashMap::new();
        for row in scored.iter().filter(|r| &r.mode == mode) {
            if let Some(score) = row.score.as_ref().filter(|s| s.normalised.reference_words > 0) {
                errors.insert(
                    (row.model.as_str(), row.clip.as_str()),
                    ClipErrors { errors: score.normalised.errors(), reference_words: score.normalised.reference_words },
                );
            }
        }
        let mut languages: Vec<String> = vec!["all".into()];
        languages.extend(plan.clips.iter().map(|c| c.language.clone()).filter(|l| l != "none").collect::<BTreeSet<_>>());
        let mut by_language = Map::new();
        for language in languages {
            let mut comparisons = Vec::new();
            for (i, a) in models.iter().enumerate() {
                for b in &models[i + 1..] {
                    let (mut va, mut vb) = (Vec::new(), Vec::new());
                    for clip in plan.clips.iter().filter(|c| language == "all" || c.language == language) {
                        if let (Some(ea), Some(eb)) = (errors.get(&(a.as_str(), clip.id.as_str())), errors.get(&(b.as_str(), clip.id.as_str()))) {
                            va.push(*ea);
                            vb.push(*eb);
                        }
                    }
                    if let Some(result) = asr_scoring::paired_bootstrap(&va, &vb, BOOTSTRAP_ITERATIONS, seed) {
                        comparisons.push(json!({
                            "a": a, "b": b, "clips": va.len(),
                            "werA": result.rate_a, "werB": result.rate_b,
                            "difference": result.difference,
                            "ci95": [result.ci95.0, result.ci95.1],
                            "pValue": result.p_value,
                        }));
                    }
                }
            }
            if !comparisons.is_empty() {
                by_language.insert(language, Value::Array(comparisons));
            }
        }
        pairwise.insert(mode.clone(), Value::Object(by_language));
    }

    // The same script read clean and under a condition.
    let primary = if plan.settings.modes.iter().any(|m| m == "app") { "app" } else { "raw" };
    let mut conditions = Map::new();
    for model in models {
        let rows: HashMap<&str, &Scored> = scored
            .iter()
            .filter(|r| &r.model == model && r.mode == primary && r.score.is_some())
            .map(|r| (r.clip.as_str(), r))
            .collect();
        let mut by_condition: BTreeMap<String, (usize, ErrorCounts, ErrorCounts)> = BTreeMap::new();
        for clip in plan.clips.iter() {
            let Some(condition) = &clip.condition else { continue };
            let (Some(variant), Some(clean)) = (rows.get(clip.id.as_str()), rows.get(base_clip(clip).as_str())) else { continue };
            let entry = by_condition.entry(condition.clone()).or_default();
            entry.0 += 1;
            entry.1.add(&clean.score.as_ref().expect("scored").normalised);
            entry.2.add(&variant.score.as_ref().expect("scored").normalised);
        }
        if !by_condition.is_empty() {
            conditions.insert(model.clone(), Value::Object(by_condition.into_iter().map(|(name, (count, clean, cond))| {
                (name, json!({"clips": count, "cleanWer": clean.rate(), "conditionWer": cond.rate()}))
            }).collect()));
        }
    }
    json!({"byModel": by_model, "pairwise": pairwise, "conditions": conditions})
}

fn page(results: &Value) -> Result<String> {
    // "<" only occurs inside strings; escaping it keeps "</script>" or "<!--"
    // in a transcript from ending the inline script.
    let json = serde_json::to_string(results)?.replace('<', "\\u003c");
    anyhow::ensure!(TEMPLATE.contains(PLACEHOLDER), "report template lacks its data placeholder");
    Ok(TEMPLATE.replacen(PLACEHOLDER, &json, 1))
}

pub fn write(dir: &Path, results: &Value) -> Result<PathBuf> {
    write_atomic(&dir.join("results.json"), serde_json::to_string_pretty(results)?.as_bytes())?;
    let html = dir.join("report.html");
    write_atomic(&html, page(results)?.as_bytes())?;
    Ok(html)
}

/// Aggregates only: no transcripts, references, terms or audio links.
pub fn strip_text(results: &Value) -> Value {
    let mut stripped = results.clone();
    stripped["textIncluded"] = json!(false);
    for clip in stripped["clips"].as_array_mut().into_iter().flatten() {
        if let Some(object) = clip.as_object_mut() {
            for key in ["reference", "segments", "terms", "audioHref"] {
                object.remove(key);
            }
        }
    }
    for row in stripped["results"].as_array_mut().into_iter().flatten() {
        if let Some(object) = row.as_object_mut() {
            object.remove("text");
            object.remove("alignment");
        }
    }
    stripped
}

pub fn export_summary(paths: &Paths, results: &Value) -> Result<(PathBuf, PathBuf)> {
    let summary = strip_text(results);
    let id = results["run"]["id"].as_str().unwrap_or("run");
    let json_path = paths.summaries.join(format!("asr-bench-{id}.json"));
    let html_path = paths.summaries.join(format!("asr-bench-{id}.html"));
    write_atomic(&json_path, serde_json::to_string_pretty(&summary)?.as_bytes())?;
    write_atomic(&html_path, page(&summary)?.as_bytes())?;
    Ok((json_path, html_path))
}

pub fn load(path: &Path) -> Result<Value> {
    let file = if path.is_dir() { path.join("results.json") } else { path.to_path_buf() };
    let text = std::fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn fmt_rate(value: &Value) -> String {
    value.as_f64().map(|v| format!("{:.1} %", v * 100.0)).unwrap_or_else(|| "—".into())
}

fn fmt_ms(value: &Value) -> String {
    match value.as_f64() {
        Some(v) if v >= 1000.0 => format!("{:.2} s", v / 1000.0),
        Some(v) => format!("{v:.0} ms"),
        None => "—".into(),
    }
}

fn primary_mode(results: &Value) -> String {
    let modes = results["run"]["settings"]["modes"].as_array().cloned().unwrap_or_default();
    if modes.iter().any(|m| m == "app") { "app".into() } else { modes.first().and_then(|m| m.as_str()).unwrap_or("raw").into() }
}

pub fn print_leaderboard(results: &Value) {
    let mode = primary_mode(results);
    let mut rows: Vec<(String, Value, Value)> = results["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model["id"].as_str()?.to_string();
            let overall = results["aggregates"]["byModel"][&id][&mode]["overall"].clone();
            (!overall.is_null()).then(|| (id, overall, model.clone()))
        })
        .collect();
    rows.sort_by(|a, b| {
        let key = |v: &Value| v["werPooled"].as_f64().unwrap_or(f64::INFINITY);
        key(&a.1).total_cmp(&key(&b.1))
    });
    println!("\nLeaderboard ({mode} mode, speech clips)");
    println!(
        "{:<24} {:>8} {:>17} {:>9} {:>11} {:>9} {:>9} {:>5}",
        "model", "WER", "95 % CI", "format.", "rel→text", "RTF", "memory", "fail"
    );
    for (id, overall, model) in rows {
        let ci = overall["ci95"]
            .as_array()
            .map(|ci| format!("{}–{}", fmt_rate(&ci[0]), fmt_rate(&ci[1])))
            .unwrap_or_else(|| "—".into());
        let memory = model["peakMemoryBytes"].as_f64().map(|b| format!("{:.2} GB", b / 1e9)).unwrap_or_else(|| "—".into());
        println!(
            "{:<24} {:>8} {:>17} {:>9} {:>11} {:>9} {:>9} {:>5}",
            id,
            fmt_rate(&overall["werPooled"]),
            ci,
            fmt_rate(&overall["formattedPooled"]),
            fmt_ms(&overall["releaseToTextP50"]),
            overall["rtfMedian"].as_f64().map(|v| format!("{v:.3}×")).unwrap_or_else(|| "—".into()),
            memory,
            overall["failures"].as_u64().unwrap_or(0),
        );
    }
}

/// Before/after table for two runs.
pub fn compare(a: &Value, b: &Value) {
    let mode = primary_mode(a);
    println!("Comparing {} → {} ({mode} mode)", a["run"]["id"].as_str().unwrap_or("A"), b["run"]["id"].as_str().unwrap_or("B"));
    println!("{:<24} {:>9} {:>9} {:>9} {:>11} {:>11}", "model", "WER A", "WER B", "Δ pp", "rel→text A", "rel→text B");
    let ids: BTreeSet<String> = a["aggregates"]["byModel"].as_object().into_iter().flatten().map(|(k, _)| k.clone()).collect();
    for id in ids {
        let (ga, gb) = (&a["aggregates"]["byModel"][&id][&mode]["overall"], &b["aggregates"]["byModel"][&id][&mode]["overall"]);
        if gb.is_null() {
            continue;
        }
        let delta = match (ga["werPooled"].as_f64(), gb["werPooled"].as_f64()) {
            (Some(x), Some(y)) => format!("{:+.1}", (y - x) * 100.0),
            _ => "—".into(),
        };
        println!(
            "{:<24} {:>9} {:>9} {:>9} {:>11} {:>11}",
            id,
            fmt_rate(&ga["werPooled"]),
            fmt_rate(&gb["werPooled"]),
            delta,
            fmt_ms(&ga["releaseToTextP50"]),
            fmt_ms(&gb["releaseToTextP50"])
        );
    }
    if a["run"]["normalisationVersion"] != b["run"]["normalisationVersion"] {
        println!("note: the runs used different normalisation versions; WER is not directly comparable.");
    }
}
