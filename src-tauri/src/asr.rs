use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio_preprocess;
use crate::settings::{LanguageMode, ModelProfile};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    pub id: String,
    pub engine: String,
    pub model_name: String,
    pub variant: String,
    pub local_path: String,
    pub size_bytes: i64,
    pub is_default: bool,
    pub profile: ModelProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub language_code: String,
    pub segment_order: i32,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptResult {
    pub job_id: String,
    pub model_name: String,
    pub full_text: String,
    pub plain_text: String,
    pub timestamped_text: String,
    pub detected_languages: Vec<String>,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSourceKind {
    QuickDictate,
    FileUpload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionPreviewRequest {
    pub source_kind: PreviewSourceKind,
    pub profile: ModelProfile,
    pub selected_model_id: Option<String>,
    pub language_mode: LanguageMode,
    pub fixed_language: Option<String>,
    pub timestamps: bool,
    pub prefer_gpu: bool,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionPreviewResponse {
    pub source_kind: PreviewSourceKind,
    pub resolved_model: Option<InstalledModel>,
    pub result: Option<TranscriptResult>,
    pub error: Option<EngineErrorPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTranscriptionRequest {
    pub profile: ModelProfile,
    pub selected_model_id: Option<String>,
    pub language_mode: LanguageMode,
    pub fixed_language: Option<String>,
    pub timestamps: bool,
    pub prefer_gpu: bool,
    pub file_path: String,
}

pub trait TranscriptionEngine: Send + Sync {
    fn list_models(&self) -> Result<Vec<InstalledModel>>;
    fn refresh_from_disk(&self) -> Result<Vec<InstalledModel>>;
    fn transcribe_file(
        &self,
        request: FileTranscriptionRequest,
        progress: Option<Arc<AtomicI32>>,
    ) -> Result<TranscriptResult>;
}

/// A loaded Whisper context kept alive between transcriptions so we don't pay
/// the (large) model-load + GPU-allocation cost on every dictation.
struct CachedContext {
    model_path: String,
    use_gpu: bool,
    context: Arc<WhisperContext>,
}

#[derive(Debug)]
pub struct SharedWhisperEngine {
    models_dir: PathBuf,
    models: Mutex<Vec<InstalledModel>>,
    // Single-slot cache: reused while the same (model, gpu) is requested,
    // rebuilt on change. See `obtain_context`.
    context_cache: Mutex<Option<CachedContext>>,
}

impl std::fmt::Debug for CachedContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedContext")
            .field("model_path", &self.model_path)
            .field("use_gpu", &self.use_gpu)
            .finish()
    }
}

impl SharedWhisperEngine {
    pub fn new(models_dir: PathBuf, models: Vec<InstalledModel>) -> Self {
        Self {
            models_dir,
            models: Mutex::new(models),
            context_cache: Mutex::new(None),
        }
    }

    /// Return a loaded context for `(model, use_gpu)`, reusing the cached one
    /// when the key matches. The cache holds a single slot; switching models or
    /// GPU mode drops the previous context first to free its memory.
    fn obtain_context(
        &self,
        model: &InstalledModel,
        use_gpu: bool,
    ) -> Result<Arc<WhisperContext>> {
        let mut cache = self
            .context_cache
            .lock()
            .map_err(|_| anyhow!("whisper context cache poisoned"))?;
        if let Some(cached) = cache.as_ref() {
            if cached.model_path == model.local_path && cached.use_gpu == use_gpu {
                return Ok(Arc::clone(&cached.context));
            }
        }
        // Drop any previous context before allocating the new one.
        *cache = None;
        let context = Arc::new(create_whisper_context(model, use_gpu)?);
        *cache = Some(CachedContext {
            model_path: model.local_path.clone(),
            use_gpu,
            context: Arc::clone(&context),
        });
        Ok(context)
    }

    fn invalidate_context_cache(&self) {
        if let Ok(mut cache) = self.context_cache.lock() {
            *cache = None;
        }
    }

    fn resolve_model(
        &self,
        selected_model_id: Option<&str>,
        profile: ModelProfile,
    ) -> Result<InstalledModel> {
        let models = self.list_models()?;
        if let Some(model_id) = selected_model_id {
            if let Some(model) = models.iter().find(|model| model.id == model_id) {
                return Ok(model.clone());
            }
        }

        models
            .iter()
            .find(|model| model.profile == profile && model.is_default)
            .or_else(|| models.iter().find(|model| model.profile == profile))
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "MODEL_MISSING: no whisper.cpp model is installed for profile {}",
                    to_profile_name(profile)
                )
            })
    }
}

impl TranscriptionEngine for SharedWhisperEngine {
    fn list_models(&self) -> Result<Vec<InstalledModel>> {
        Ok(self
            .models
            .lock()
            .map_err(|_| anyhow!("model registry poisoned"))?
            .clone())
    }

    fn refresh_from_disk(&self) -> Result<Vec<InstalledModel>> {
        let refreshed = discover_whisper_models(&self.models_dir)?;
        *self
            .models
            .lock()
            .map_err(|_| anyhow!("model registry poisoned"))? = refreshed.clone();
        // A model file may have been replaced/removed; drop the cached context
        // so the next transcription reloads from disk.
        self.invalidate_context_cache();
        Ok(refreshed)
    }

    fn transcribe_file(
        &self,
        request: FileTranscriptionRequest,
        progress: Option<Arc<AtomicI32>>,
    ) -> Result<TranscriptResult> {
        let model = self.resolve_model(request.selected_model_id.as_deref(), request.profile)?;
        let prepared = audio_preprocess::decode_audio_file(Path::new(&request.file_path))
            .with_context(|| format!("failed to prepare {}", request.file_path))?;
        let use_gpu = should_try_gpu(request.prefer_gpu);

        let (context, gpu_active) = if use_gpu {
            match self.obtain_context(&model, true) {
                Ok(ctx) => (ctx, true),
                Err(_) => (self.obtain_context(&model, false)?, false),
            }
        } else {
            (self.obtain_context(&model, false)?, false)
        };

        let transcript = run_whisper(context.as_ref(), &model, &prepared, &request, &progress)
            .or_else(|error| {
                if gpu_active {
                    let cpu_context = self
                        .obtain_context(&model, false)
                        .with_context(|| format!("{}; CPU context creation also failed", error))?;
                    run_whisper(cpu_context.as_ref(), &model, &prepared, &request, &progress)
                        .with_context(|| format!("{}; CPU fallback also failed", error))
                } else {
                    Err(error)
                }
            })?;
        Ok(transcript)
    }
}

pub fn discover_whisper_models(models_dir: &Path) -> Result<Vec<InstalledModel>> {
    fs::create_dir_all(models_dir)?;
    let mut models = Vec::new();
    for entry in fs::read_dir(models_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("bin") {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.len() < 1024 * 1024 {
            continue;
        }

        let model_name = entry.file_name().to_string_lossy().to_string();
        let profile = profile_for_model_name(&model_name);
        models.push(InstalledModel {
            id: model_id_from_name(&model_name),
            engine: "whisper.cpp".to_string(),
            variant: to_profile_name(profile).to_string(),
            model_name,
            local_path: path.display().to_string(),
            size_bytes: metadata.len() as i64,
            is_default: is_default_model(&path),
            profile,
        });
    }

    models.sort_by(|left, right| left.model_name.cmp(&right.model_name));
    Ok(models)
}

fn create_whisper_context(model: &InstalledModel, use_gpu: bool) -> Result<WhisperContext> {
    let context_params = WhisperContextParameters {
        use_gpu,
        ..WhisperContextParameters::default()
    };
    WhisperContext::new_with_params(&model.local_path, context_params)
        .with_context(|| format!("failed to load model {}", model.model_name))
}

fn run_whisper(
    context: &WhisperContext,
    model: &InstalledModel,
    prepared: &audio_preprocess::PreparedAudio,
    request: &FileTranscriptionRequest,
    progress: &Option<Arc<AtomicI32>>,
) -> Result<TranscriptResult> {
    let first_attempt = run_whisper_once(context, model, prepared, request, None, progress)?;
    if !first_attempt.segments.is_empty() {
        return Ok(first_attempt);
    }

    let detected_language = first_attempt
        .detected_languages
        .first()
        .cloned()
        .filter(|language| language != "unknown");

    if matches!(request.language_mode, LanguageMode::Auto) {
        if let Some(language) = detected_language.clone() {
            let retry =
                run_whisper_once(&context, model, prepared, request, Some(language), progress)?;
            if !retry.segments.is_empty() {
                return Ok(retry);
            }
        }
    }

    if !request.timestamps {
        let mut timestamp_retry_request = request.clone();
        timestamp_retry_request.timestamps = true;
        let retry = run_whisper_once(
            &context,
            model,
            prepared,
            &timestamp_retry_request,
            detected_language.clone(),
            progress,
        )?;
        if !retry.segments.is_empty() {
            return Ok(retry);
        }
    }

    Err(anyhow!("TRANSCRIPTION_EMPTY: whisper produced no segments"))
}

fn run_whisper_once(
    context: &WhisperContext,
    model: &InstalledModel,
    prepared: &audio_preprocess::PreparedAudio,
    request: &FileTranscriptionRequest,
    forced_language: Option<String>,
    progress: &Option<Arc<AtomicI32>>,
) -> Result<TranscriptResult> {
    let mut state = context
        .create_state()
        .context("failed to create whisper state")?;
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().min(8) as i32)
        .unwrap_or(4);
    params.set_n_threads(threads);
    params.set_translate(false);
    params.set_no_context(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_timestamps(!request.timestamps);

    // Anti-hallucination / repetition-loop settings
    params.set_temperature(0.0);
    params.set_temperature_inc(0.2); // retry with increasing temperature on fallback
    params.set_entropy_thold(2.4); // trigger fallback when token entropy is low (repetitive)
    params.set_suppress_blank(true);
    params.set_suppress_nst(true); // suppress non-speech tokens like "[Music]", "Subtitles by..."
    params.set_n_max_text_ctx(64); // limit past context to prevent loop propagation

    if let Some(progress_atomic) = progress.clone() {
        params.set_progress_callback_safe(move |pct: i32| {
            progress_atomic.store(pct, Ordering::Relaxed);
        });
    }

    match request.language_mode {
        LanguageMode::Auto => {
            if let Some(language) = forced_language.as_deref() {
                params.set_language(Some(language));
                params.set_detect_language(false);
            } else {
                params.set_language(None);
                params.set_detect_language(true);
            }
        }
        LanguageMode::Fixed => {
            params.set_language(request.fixed_language.as_deref());
            params.set_detect_language(false);
        }
    }

    state
        .full(params, &prepared.samples)
        .context("failed to run whisper transcription")?;

    let detected_language = forced_language
        .or_else(|| {
            request
                .fixed_language
                .clone()
                .filter(|_| matches!(request.language_mode, LanguageMode::Fixed))
        })
        .or_else(|| {
            whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());

    let job_id = Uuid::new_v4().to_string();
    let mut segments = Vec::new();

    for (index, segment) in state.as_iter().enumerate() {
        let text = segment.to_string().trim().to_string();
        if text.is_empty() {
            continue;
        }
        let start_ms = timestamp_units_to_ms(segment.start_timestamp());
        let end_ms = timestamp_units_to_ms(segment.end_timestamp());
        let speech_confidence = (1.0 - segment.no_speech_probability()).clamp(0.0, 1.0);

        // Skip segments where Whisper is 80%+ confident there is no speech —
        // these are the most likely source of hallucinated / repeated text.
        if speech_confidence < 0.2 {
            continue;
        }

        segments.push(TranscriptSegment {
            id: format!("{job_id}:{index}"),
            start_ms,
            end_ms,
            text,
            language_code: detected_language.clone(),
            segment_order: index as i32,
            confidence: Some(speech_confidence),
        });
    }

    // Post-processing: remove hallucinated repetition loops.
    // If 3+ consecutive segments have identical text, keep only the first.
    let segments = deduplicate_segments(segments);

    let plain_text = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let timestamped_text = segments
        .iter()
        .map(|s| {
            format!(
                "[{} - {}] {}: {}",
                format_ms(s.start_ms),
                format_ms(s.end_ms),
                s.language_code,
                s.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let detected_languages = {
        let mut set = HashSet::new();
        set.insert(detected_language.clone());
        set.into_iter().collect::<Vec<_>>()
    };

    Ok(TranscriptResult {
        job_id,
        model_name: model.model_name.clone(),
        full_text: plain_text.clone(),
        plain_text,
        timestamped_text,
        detected_languages,
        segments,
    })
}

/// Remove hallucinated repetition loops from segments.
///
/// If 3 or more consecutive segments contain the same text (after normalization),
/// only the first occurrence is kept. This catches decoder loops that the
/// temperature-fallback and entropy checks didn't prevent.
fn deduplicate_segments(segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    if segments.len() < 3 {
        return segments;
    }

    fn normalize(text: &str) -> String {
        text.trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    // First pass: identify runs of consecutive identical segments.
    // For each segment, record how many consecutive repeats follow it (including itself).
    let normalized: Vec<String> = segments.iter().map(|s| normalize(&s.text)).collect();
    let mut run_lengths = vec![1usize; segments.len()];
    let mut i = segments.len() - 1;
    while i > 0 {
        i -= 1;
        if normalized[i] == normalized[i + 1] {
            run_lengths[i] = run_lengths[i + 1] + 1;
        }
    }

    // Second pass: keep only the first segment of any run of 3+.
    let mut result = Vec::with_capacity(segments.len());
    let mut skip_until = 0usize;
    for (idx, segment) in segments.into_iter().enumerate() {
        if idx < skip_until {
            continue;
        }
        if run_lengths[idx] >= 3 {
            // Keep this segment (the first in the run), skip the rest.
            skip_until = idx + run_lengths[idx];
        }
        result.push(segment);
    }

    // Re-number segment_order after deduplication.
    for (order, segment) in result.iter_mut().enumerate() {
        segment.segment_order = order as i32;
    }

    result
}

fn should_try_gpu(prefer_gpu: bool) -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        prefer_gpu
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = prefer_gpu;
        false
    }
}

fn model_id_from_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('.', "-")
}

fn profile_for_model_name(name: &str) -> ModelProfile {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("tiny") {
        ModelProfile::Fast
    } else if normalized.contains("small") {
        ModelProfile::Balanced
    } else {
        ModelProfile::Accurate
    }
}

fn is_default_model(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    matches!(
        name,
        "ggml-small.bin" | "ggml-medium.bin" | "ggml-large-v3-turbo-q5_0.bin"
    )
}

fn to_profile_name(profile: ModelProfile) -> &'static str {
    match profile {
        ModelProfile::Fast => "fast",
        ModelProfile::Balanced => "balanced",
        ModelProfile::Accurate => "accurate",
    }
}

fn timestamp_units_to_ms(value: i64) -> i64 {
    value * 10
}

fn format_ms(ms: i64) -> String {
    let total_seconds = ms.max(0) / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}
