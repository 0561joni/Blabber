//! The child side: one process loads one model and transcribes clips with it,
//! reporting newline-delimited JSON events on stdout.
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use speech_to_text_lib::{
    asr::{self, FileTranscriptionRequest, TranscriptResult, TranscriptionEngine},
    audio_preprocess::{self, PreparedAudio},
    live_pair::HeadlessLivePair,
    model_metadata::ModelUseContext,
    r2t2::{HeadlessR2t2, HeadlessRun},
    settings::LanguageMode,
    transcription_policy::MAX_CHUNK_MS,
    vocabulary,
};

use crate::models::{BenchModel, Engine};

/// Early ASR commits a window once this much audio waits (`early_asr.rs`).
const EARLY_WINDOW_TRIGGER_MS: u64 = MAX_CHUNK_MS as u64 + 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildClip {
    pub id: String,
    pub wav: PathBuf,
    pub duration_ms: u64,
    pub category: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSpec {
    pub name: String,
    pub prompt: Option<String>,
    pub prompt_terms: Vec<String>,
    pub correct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildJob {
    pub model: BenchModel,
    pub models_dir: PathBuf,
    pub db_path: PathBuf,
    pub temp_dir: PathBuf,
    pub clips: Vec<ChildClip>,
    pub modes: Vec<ModeSpec>,
    pub repeats: u32,
    pub long_repeats: u32,
    pub stream_repeats: u32,
    pub fixed_language: bool,
    /// Draft references: file-transcription settings with timestamps, raw text.
    pub draft: bool,
    /// Skip the load probe when resuming after a crash.
    pub measure_load: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    Loaded {
        cold_ms: Option<u64>,
        warm_ms: Option<u64>,
        per_transcription: bool,
    },
    #[serde(rename_all = "camelCase")]
    Started { clip: String, runs: u32 },
    #[serde(rename_all = "camelCase")]
    Clip(ClipOutput),
    #[serde(rename_all = "camelCase")]
    ClipError { clip: String, mode: Option<String>, message: String },
    Done,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipOutput {
    pub clip: String,
    pub mode: String,
    pub text: String,
    /// Live preview text at release (live pair).
    pub stream_text: Option<String>,
    pub detected_language: Option<String>,
    /// Wall time per run: batch = transcription; streaming paced = release → final.
    pub runs_ms: Vec<u64>,
    pub compute_ms: Option<u64>,
    pub early_ms: Option<u64>,
    pub first_word_ms: Vec<u64>,
    pub warning: Option<String>,
    pub segments: Vec<Segment>,
}

fn emit(event: &Event) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, event);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn median(values: &[u64]) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[n / 2]),
        n => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2),
    }
}

fn ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

/// A minimal transcript for streaming text, so vocabulary correction runs on
/// the same type the app corrects.
fn transcript_for(text: &str, duration_ms: u64, language: Option<&str>) -> Result<TranscriptResult> {
    let language = language.unwrap_or("und");
    Ok(serde_json::from_value(serde_json::json!({
        "jobId": "bench", "modelName": "bench", "fullText": text, "plainText": text,
        "timestampedText": text, "detectedLanguages": [language], "qualityStatus": "clean",
        "recoveredRegionCount": 0, "warnings": [],
        "segments": [{"id": "bench:0", "startMs": 0, "endMs": duration_ms, "text": text,
            "languageCode": language, "segmentOrder": 0, "confidence": null}]
    }))?)
}

fn correct(job: &ChildJob, mode: &ModeSpec, text: &str, duration_ms: u64, language: Option<&str>) -> Result<String> {
    if !mode.correct || text.trim().is_empty() {
        return Ok(text.trim().to_string());
    }
    let corrected = vocabulary::correct_transcript_result(&job.db_path, transcript_for(text, duration_ms, language)?)?;
    Ok(corrected.plain_text.trim().to_string())
}

/// Groups modes that send the engine the same prompt, so it runs once per group.
fn prompt_groups(modes: &[ModeSpec], uses_prompt: bool) -> Vec<Vec<&ModeSpec>> {
    let mut groups: Vec<Vec<&ModeSpec>> = Vec::new();
    for mode in modes {
        let key = |m: &ModeSpec| if uses_prompt { m.prompt.clone() } else { None };
        match groups.iter_mut().find(|group| key(group[0]) == key(mode)) {
            Some(group) => group.push(mode),
            None => groups.push(vec![mode]),
        }
    }
    groups
}

fn runs_for(job: &ChildJob, clip: &ChildClip, streaming: bool) -> u32 {
    if job.draft {
        1
    } else if clip.category == "long" {
        job.long_repeats.max(1)
    } else if streaming {
        job.stream_repeats.max(1)
    } else {
        job.repeats.max(1)
    }
}

fn write_slice(job: &ChildJob, name: &str, audio: &PreparedAudio, start: usize, end: usize) -> Result<PathBuf> {
    let path = job.temp_dir.join(format!("{}-{name}.wav", std::process::id()));
    audio_preprocess::write_wav(
        &path,
        &PreparedAudio {
            sample_rate_hz: 16_000,
            channels: 1,
            samples: audio.samples[start.min(audio.samples.len())..end.min(audio.samples.len())].to_vec(),
        },
    )?;
    Ok(path)
}

pub fn run_child() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    let job: ChildJob = serde_json::from_str(&input).context("invalid child job")?;
    std::fs::create_dir_all(&job.temp_dir)?;
    let result = match &job.model.engine {
        Engine::Batch { installed } => run_batch(&job, installed),
        Engine::R2t2 { helper, model } => run_r2t2(&job, helper, model),
        Engine::LivePair { helper, nemotron, parakeet, chunk_ms } => {
            run_live_pair(&job, helper, nemotron, parakeet, *chunk_ms)
        }
    };
    if let Err(error) = &result {
        emit(&Event::ClipError { clip: String::new(), mode: None, message: format!("{error:#}") });
    }
    emit(&Event::Done);
    result
}

// MARK: Batch engines

fn batch_request(job: &ChildJob, installed: &asr::InstalledModel, clip: &ChildClip, path: &Path, mode: Option<&ModeSpec>) -> FileTranscriptionRequest {
    let dictation = clip.category == "short" && !job.draft;
    let supports_dictation = installed.capabilities.supported_contexts.contains(&ModelUseContext::ShortcutDictation);
    let (use_context, timestamps) = if dictation && supports_dictation {
        (ModelUseContext::ShortcutDictation, false)
    } else {
        (ModelUseContext::FileTranscription, true)
    };
    let fixed = job.fixed_language && matches!(clip.language.as_str(), "de" | "en" | "fr");
    FileTranscriptionRequest {
        use_context: Some(use_context),
        profile: installed.profile,
        selected_model_id: Some(installed.id.clone()),
        language_mode: if fixed { LanguageMode::Fixed } else { LanguageMode::Auto },
        fixed_language: fixed.then(|| clip.language.clone()),
        timestamps,
        prefer_gpu: true,
        file_path: path.display().to_string(),
        context_prompt: mode.and_then(|m| m.prompt.clone()),
        context_terms: mode.map(|m| m.prompt_terms.clone()).unwrap_or_default(),
    }
}

fn run_batch(job: &ChildJob, installed: &asr::InstalledModel) -> Result<()> {
    let engine = asr::LocalTranscriptionEngine::new(job.models_dir.clone(), vec![installed.clone()]);
    let preloadable = matches!(installed.engine.as_str(), "whisper.cpp" | "qwen3_asr_c");
    let first = job.clips.first().ok_or_else(|| anyhow!("no clips"))?;
    let first_audio = audio_preprocess::decode_audio_file(&first.wav)?;
    let warmup = write_slice(job, "warmup", &first_audio, 0, 16_000 * 4)?;
    let template = batch_request(job, installed, first, &warmup, None);

    let (mut cold_ms, mut warm_ms) = (None, None);
    if preloadable {
        let began = Instant::now();
        engine.preload(&template)?;
        if job.measure_load {
            cold_ms = Some(ms(began.elapsed()));
            engine.release_resources();
            let began = Instant::now();
            engine.preload(&template)?;
            warm_ms = Some(ms(began.elapsed()));
        }
    }
    // First decode compiles GPU kernels and warms caches; it is not measured.
    let _ = engine.transcribe_file(template, None);
    let _ = std::fs::remove_file(&warmup);
    emit(&Event::Loaded { cold_ms, warm_ms, per_transcription: !preloadable });

    for clip in &job.clips {
        let runs = runs_for(job, clip, false);
        emit(&Event::Started { clip: clip.id.clone(), runs: runs * job.modes.len() as u32 });
        for group in prompt_groups(&job.modes, installed.capabilities.context_support) {
            let request = batch_request(job, installed, clip, &clip.wav, Some(group[0]));
            let outcome = (|| -> Result<(TranscriptResult, Vec<u64>, Option<u64>)> {
                let mut times = Vec::new();
                let mut first = None;
                for _ in 0..runs {
                    let began = Instant::now();
                    let result = match engine.transcribe_file(request.clone(), None) {
                        // "No speech" is an answer, not a failure (silence clips).
                        Err(error) if error.to_string().starts_with("TRANSCRIPTION_EMPTY") => {
                            transcript_for("", clip.duration_ms, None)?
                        }
                        other => other?,
                    };
                    times.push(ms(began.elapsed()));
                    first.get_or_insert(result);
                }
                let early = early_tail_ms(job, &engine, installed, clip, preloadable, group[0])?;
                Ok((first.expect("at least one run"), times, early))
            })();
            match outcome {
                Ok((result, times, early)) => {
                    let language = result.detected_languages.first().cloned();
                    let segments: Vec<Segment> = result
                        .segments
                        .iter()
                        .map(|s| Segment { start_ms: s.start_ms, end_ms: s.end_ms, text: s.text.trim().to_string() })
                        .collect();
                    for mode in group {
                        let text = if mode.correct && !result.plain_text.trim().is_empty() {
                            vocabulary::correct_transcript_result(&job.db_path, result.clone())
                                .map(|r| r.plain_text)
                                .unwrap_or_else(|_| result.plain_text.clone())
                        } else {
                            result.plain_text.clone()
                        };
                        emit(&Event::Clip(ClipOutput {
                            clip: clip.id.clone(),
                            mode: mode.name.clone(),
                            text: text.trim().to_string(),
                            detected_language: language.clone(),
                            compute_ms: median(&times),
                            runs_ms: times.clone(),
                            early_ms: early,
                            warning: result.warnings.first().map(|w| w.reason.clone()),
                            segments: if job.draft { segments.clone() } else { Vec::new() },
                            ..Default::default()
                        }));
                    }
                }
                Err(error) => {
                    for mode in group {
                        emit(&Event::ClipError {
                            clip: clip.id.clone(),
                            mode: Some(mode.name.clone()),
                            message: format!("{error:#}"),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Early ASR transcribes full windows while the user speaks; on release only
/// the tail is decoded. Simulated for short clips long enough to commit one.
fn early_tail_ms(
    job: &ChildJob,
    engine: &asr::LocalTranscriptionEngine,
    installed: &asr::InstalledModel,
    clip: &ChildClip,
    preloadable: bool,
    mode: &ModeSpec,
) -> Result<Option<u64>> {
    if job.draft || !preloadable || clip.category != "short" || clip.duration_ms <= EARLY_WINDOW_TRIGGER_MS {
        return Ok(None);
    }
    let mut committed_ms = 0;
    while clip.duration_ms - committed_ms > EARLY_WINDOW_TRIGGER_MS {
        committed_ms += MAX_CHUNK_MS as u64;
    }
    let audio = audio_preprocess::decode_audio_file(&clip.wav)?;
    let tail = write_slice(job, "tail", &audio, committed_ms as usize * 16, audio.samples.len())?;
    let began = Instant::now();
    let result = engine.transcribe_file(batch_request(job, installed, clip, &tail, Some(mode)), None);
    let elapsed = ms(began.elapsed());
    let _ = std::fs::remove_file(&tail);
    result.map(|_| Some(elapsed))
}

// MARK: Streaming engines

fn emit_streaming(job: &ChildJob, clip: &ChildClip, group: &[&ModeSpec], paced: &[HeadlessRun], unpaced: Option<&HeadlessRun>) -> Result<()> {
    let reference = paced.first().or(unpaced).ok_or_else(|| anyhow!("no run"))?;
    let language = reference.language.as_deref();
    for mode in group {
        let stream_text = match &reference.stream_text {
            Some(text) => Some(correct(job, mode, text, clip.duration_ms, language)?),
            None => None,
        };
        emit(&Event::Clip(ClipOutput {
            clip: clip.id.clone(),
            mode: mode.name.clone(),
            text: correct(job, mode, &reference.text, clip.duration_ms, language)?,
            stream_text,
            detected_language: reference.language.clone(),
            runs_ms: paced.iter().filter_map(|run| run.release_to_text_ms).collect(),
            compute_ms: unpaced.map(|run| run.total_ms),
            early_ms: None,
            first_word_ms: paced.iter().filter_map(|run| run.first_text_ms).collect(),
            warning: reference.warning.clone(),
            segments: Vec::new(),
        }));
    }
    Ok(())
}

/// Paced runs measure dictation latency on short clips; one unpaced run
/// measures throughput. Long clips are unpaced only.
fn stream_clip(
    job: &ChildJob,
    clip: &ChildClip,
    group: &[&ModeSpec],
    transcribe: &mut dyn FnMut(&[f32], bool) -> Result<HeadlessRun>,
) -> Result<()> {
    let audio = audio_preprocess::decode_audio_file(&clip.wav)?;
    let mut paced = Vec::new();
    if clip.category == "short" && !job.draft {
        for _ in 0..runs_for(job, clip, true) {
            paced.push(transcribe(&audio.samples, true)?);
        }
    }
    let unpaced = transcribe(&audio.samples, false)?;
    emit_streaming(job, clip, group, &paced, Some(&unpaced))
}

fn streaming_loop(
    job: &ChildJob,
    uses_prompt: bool,
    mut transcribe: impl FnMut(&ChildClip, &ModeSpec, &[f32], bool) -> Result<HeadlessRun>,
) -> Result<()> {
    for clip in &job.clips {
        let runs = if clip.category == "short" { runs_for(job, clip, true) + 1 } else { 1 };
        emit(&Event::Started { clip: clip.id.clone(), runs: runs * job.modes.len() as u32 });
        for group in prompt_groups(&job.modes, uses_prompt) {
            let mode = group[0];
            let result = stream_clip(job, clip, &group, &mut |samples, paced| transcribe(clip, mode, samples, paced));
            if let Err(error) = result {
                for mode in &group {
                    emit(&Event::ClipError { clip: clip.id.clone(), mode: Some(mode.name.clone()), message: format!("{error:#}") });
                }
            }
        }
    }
    Ok(())
}

fn first_seconds(job: &ChildJob, seconds: usize) -> Result<Vec<f32>> {
    let first = job.clips.first().ok_or_else(|| anyhow!("no clips"))?;
    let mut samples = audio_preprocess::decode_audio_file(&first.wav)?.samples;
    samples.truncate(16_000 * seconds);
    Ok(samples)
}

fn r2t2_language(job: &ChildJob, clip: &ChildClip) -> &'static str {
    match (job.fixed_language, clip.language.as_str()) {
        (true, "de") => "German",
        (true, "en") => "English",
        _ => "",
    }
}

fn run_r2t2(job: &ChildJob, helper: &Path, model: &Path) -> Result<()> {
    let cold = if job.measure_load {
        let (first, load) = HeadlessR2t2::load(helper, model, false)?;
        drop(first);
        Some(ms(load))
    } else {
        None
    };
    let (mut worker, load) = HeadlessR2t2::load(helper, model, false)?;
    worker.transcribe(&first_seconds(job, 3)?, "", "", false)?;
    emit(&Event::Loaded {
        cold_ms: cold,
        warm_ms: job.measure_load.then(|| ms(load)),
        per_transcription: false,
    });
    streaming_loop(job, true, |clip, mode, samples, paced| {
        worker.transcribe(samples, r2t2_language(job, clip), mode.prompt.as_deref().unwrap_or(""), paced)
    })
}

fn run_live_pair(job: &ChildJob, helper: &Path, nemotron: &Path, parakeet: &Path, chunk_ms: u32) -> Result<()> {
    let cold = if job.measure_load {
        let (first, load) = HeadlessLivePair::load(helper, nemotron, parakeet, chunk_ms)?;
        drop(first);
        Some(load.load_ms)
    } else {
        None
    };
    let (mut pair, load) = HeadlessLivePair::load(helper, nemotron, parakeet, chunk_ms)?;
    pair.transcribe(&first_seconds(job, 3)?, "auto", false)?;
    emit(&Event::Loaded {
        cold_ms: cold,
        warm_ms: job.measure_load.then_some(load.load_ms),
        per_transcription: false,
    });
    streaming_loop(job, false, |clip, _, samples, paced| {
        let language = if job.fixed_language && matches!(clip.language.as_str(), "de" | "en" | "fr") {
            clip.language.as_str()
        } else {
            "auto"
        };
        pair.transcribe(samples, language, paced)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(name: &str, prompt: Option<&str>) -> ModeSpec {
        ModeSpec { name: name.into(), prompt: prompt.map(Into::into), prompt_terms: vec![], correct: name == "app" }
    }

    #[test]
    fn modes_share_a_run_when_the_engine_sees_the_same_prompt() {
        let modes = [mode("raw", None), mode("app", Some("terms"))];
        assert_eq!(prompt_groups(&modes, true).len(), 2);
        assert_eq!(prompt_groups(&modes, false).len(), 1);
        let same = [mode("raw", None), mode("app", None)];
        assert_eq!(prompt_groups(&same, true).len(), 1);
    }

    #[test]
    fn medians_handle_even_and_odd_counts() {
        assert_eq!(median(&[3, 1, 2]), Some(2));
        assert_eq!(median(&[4, 1, 3, 2]), Some(2));
        assert_eq!(median(&[]), None);
    }
}
