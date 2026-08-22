use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::asr::{
    InstalledModel, TranscriptQualityStatus, TranscriptResult, TranscriptSegment, TranscriptWarning,
};
use crate::audio_files::SelectedSourceFile;
use crate::diarization::{
    DiarizationSource, DiarizationStatus, DiarizationTurn, TranscriptSpeaker,
};
use crate::settings::{
    AppSettings, DefaultMode, InsertBehavior, LanguageMode, ModelProfile, SettingsPatch,
    ShortcutMode,
};
use crate::speaker_reconciliation::SpeakerAttribution;

const INIT_MIGRATION: &str = include_str!("../migrations/001_init.sql");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    QuickDictate,
    FileUpload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptStatus {
    Queued,
    Recording,
    Processing,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    pub id: String,
    pub created_at: String,
    pub source_type: SourceType,
    pub title: String,
    pub plain_text: String,
    pub status: TranscriptStatus,
    pub detected_languages: Vec<String>,
    pub duration_ms: Option<i64>,
    pub model_name: Option<String>,
    pub quality_status: TranscriptQualityStatus,
    pub recovered_region_count: i32,
    pub diarization_status: DiarizationStatus,
    pub speaker_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDetail {
    #[serde(flatten)]
    pub summary: TranscriptSummary,
    pub full_text: String,
    pub timestamped_text: String,
    pub transcription_warnings: Vec<TranscriptWarning>,
    pub diarization_model_id: Option<String>,
    pub diarization_source: DiarizationSource,
    pub diarization_warning: Option<String>,
    pub diarization_policy_version: Option<u32>,
    pub diarization_clustering_threshold: Option<f32>,
    pub diarization_speaker_count_hint: Option<i32>,
    pub segments: Vec<TranscriptSegment>,
    pub speakers: Vec<TranscriptSpeaker>,
    pub diarization_turns: Vec<DiarizationTurn>,
}

#[derive(Debug, Clone)]
pub struct FileTranscriptionPerformance {
    pub avg_audio_ms_per_wall_ms: f64,
    pub sample_count: i64,
}

pub fn initialize_database(state: &AppState) -> Result<()> {
    if let Some(parent) = state.db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = open_connection(state)?;
    connection
        .execute_batch(INIT_MIGRATION)
        .context("failed to run initial migration")?;
    ensure_settings_columns(&connection)?;
    ensure_transcript_quality_columns(&connection)?;
    ensure_diarization_schema(&connection)?;
    ensure_vocabulary_columns(&connection)?;
    ensure_file_transcription_performance_table(&connection)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_migrations (
            migration_key TEXT PRIMARY KEY,
            completed_at TEXT NOT NULL
        );",
    )?;
    seed_default_settings(&connection)?;
    Ok(())
}

const RETIRE_WHISPER_TINY_MIGRATION: &str = "retire_whisper_tiny_v1";

pub fn retire_whisper_tiny(state: &AppState, installed_models: &[InstalledModel]) -> Result<bool> {
    let connection = open_connection(state)?;
    let completed = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM app_migrations WHERE migration_key=?1)",
        [RETIRE_WHISPER_TINY_MIGRATION],
        |row| row.get::<_, bool>(0),
    )?;
    if completed {
        return Ok(false);
    }

    let current = get_settings(state)?;
    let fallback_for = |preferences: &[&str], profile: ModelProfile| {
        find_model_by_name(installed_models, preferences)
            .or_else(|| resolve_profile_model(installed_models, profile))
    };
    let mut patch = SettingsPatch::default();
    if is_tiny_selection(current.shortcut_dictation_selected_model_id.as_deref()) {
        let fallback = fallback_for(shortcut_model_preferences(), fallback_shortcut_profile());
        patch.shortcut_dictation_model_profile = Some(
            fallback
                .as_ref()
                .map(|model| model.profile)
                .unwrap_or(ModelProfile::Balanced),
        );
        patch.shortcut_dictation_selected_model_id =
            Some(fallback.as_ref().map(|model| model.id.clone()));
    }
    if is_tiny_selection(current.quick_dictate_selected_model_id.as_deref()) {
        let fallback = fallback_for(
            quick_dictate_model_preferences(),
            fallback_quick_dictate_profile(),
        );
        patch.quick_dictate_model_profile = Some(
            fallback
                .as_ref()
                .map(|model| model.profile)
                .unwrap_or(ModelProfile::Balanced),
        );
        patch.quick_dictate_selected_model_id =
            Some(fallback.as_ref().map(|model| model.id.clone()));
    }
    if is_tiny_selection(current.file_transcribe_selected_model_id.as_deref()) {
        let fallback = fallback_for(
            file_transcribe_model_preferences(),
            fallback_file_transcribe_profile(),
        );
        patch.file_transcribe_model_profile = Some(
            fallback
                .as_ref()
                .map(|model| model.profile)
                .unwrap_or(ModelProfile::Balanced),
        );
        patch.file_transcribe_selected_model_id =
            Some(fallback.as_ref().map(|model| model.id.clone()));
    }
    let _ = update_settings(state, patch)?;

    retire_whisper_tiny_files(&state.models_dir)?;
    connection.execute(
        "INSERT INTO app_migrations (migration_key, completed_at) VALUES (?1, ?2)",
        params![RETIRE_WHISPER_TINY_MIGRATION, Utc::now().to_rfc3339()],
    )?;
    Ok(true)
}

fn is_tiny_selection(selection: Option<&str>) -> bool {
    selection.is_some_and(|id| id.to_ascii_lowercase().starts_with("ggml-tiny"))
}

fn retire_whisper_tiny_files(models_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(models_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.parent() != Some(models_dir) || !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let tiny_weight = name.starts_with("ggml-tiny") && name.ends_with(".bin");
        let tiny_partial = name.starts_with("ggml-tiny") && name.ends_with(".bin.part");
        if tiny_weight || tiny_partial {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn sync_installed_models(state: &AppState, models: &[InstalledModel]) -> Result<()> {
    sync_installed_models_for_db_path(&state.db_path, models)
}

pub fn sync_installed_models_for_db_path(db_path: &Path, models: &[InstalledModel]) -> Result<()> {
    let mut connection = open_connection_by_path(db_path)?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM installed_models", [])?;
    for model in models {
        transaction.execute(
            "INSERT INTO installed_models (id, engine, model_name, variant, local_path, size_bytes, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &model.id,
                &model.engine,
                &model.model_name,
                &model.variant,
                &model.local_path,
                model.size_bytes,
                model.is_default,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn apply_preferred_model_defaults(state: &AppState, models: &[InstalledModel]) -> Result<()> {
    apply_preferred_model_defaults_for_db_path(&state.db_path, models)
}

#[cfg(target_os = "windows")]
pub fn replace_qwen_selections_with_whisper(
    db_path: &Path,
    models: &[InstalledModel],
) -> Result<bool> {
    let current = get_settings_from_db_path(db_path)?;
    let qwen_id = crate::qwen_asr::QWEN_MODEL_ID;
    let selected_qwen = current.shortcut_dictation_selected_model_id.as_deref() == Some(qwen_id)
        || current.quick_dictate_selected_model_id.as_deref() == Some(qwen_id)
        || current.file_transcribe_selected_model_id.as_deref() == Some(qwen_id);
    if !selected_qwen {
        return Ok(false);
    }

    let fallback = models
        .iter()
        .find(|model| {
            model.engine == "whisper.cpp"
                && model.profile == ModelProfile::Accurate
                && model.is_default
        })
        .or_else(|| {
            models.iter().find(|model| {
                model.engine == "whisper.cpp" && model.profile == ModelProfile::Accurate
            })
        });
    let mut patch = SettingsPatch::default();
    let fallback_id = fallback.map(|model| model.id.clone());
    if current.shortcut_dictation_selected_model_id.as_deref() == Some(qwen_id) {
        patch.shortcut_dictation_model_profile = Some(ModelProfile::Accurate);
        patch.shortcut_dictation_selected_model_id = Some(fallback_id.clone());
    }
    if current.quick_dictate_selected_model_id.as_deref() == Some(qwen_id) {
        patch.quick_dictate_model_profile = Some(ModelProfile::Accurate);
        patch.quick_dictate_selected_model_id = Some(fallback_id.clone());
    }
    if current.file_transcribe_selected_model_id.as_deref() == Some(qwen_id) {
        patch.file_transcribe_model_profile = Some(ModelProfile::Accurate);
        patch.file_transcribe_selected_model_id = Some(fallback_id);
    }
    let _ = update_settings_for_db_path(db_path, patch)?;
    Ok(true)
}

pub fn apply_preferred_model_defaults_for_db_path(
    db_path: &Path,
    models: &[InstalledModel],
) -> Result<()> {
    let current = get_settings_from_db_path(db_path)?;
    let mut patch = SettingsPatch::default();
    let mut should_update = false;

    if current
        .shortcut_dictation_selected_model_id
        .as_deref()
        .and_then(|model_id| find_model_by_id(models, model_id))
        .is_none()
    {
        if let Some(model) = find_model_by_name(models, shortcut_model_preferences())
            .or_else(|| resolve_profile_model(models, fallback_shortcut_profile()))
        {
            patch.shortcut_dictation_model_profile = Some(model.profile);
            patch.shortcut_dictation_selected_model_id = Some(Some(model.id.clone()));
            should_update = true;
        }
    }

    if current
        .quick_dictate_selected_model_id
        .as_deref()
        .and_then(|model_id| find_model_by_id(models, model_id))
        .is_none()
    {
        if let Some(model) = find_model_by_name(models, quick_dictate_model_preferences())
            .or_else(|| resolve_profile_model(models, fallback_quick_dictate_profile()))
        {
            patch.quick_dictate_model_profile = Some(model.profile);
            patch.quick_dictate_selected_model_id = Some(Some(model.id.clone()));
            should_update = true;
        }
    }

    if current
        .file_transcribe_selected_model_id
        .as_deref()
        .and_then(|model_id| find_model_by_id(models, model_id))
        .is_none()
    {
        if let Some(model) = find_model_by_name(models, file_transcribe_model_preferences())
            .or_else(|| resolve_profile_model(models, fallback_file_transcribe_profile()))
        {
            patch.file_transcribe_model_profile = Some(model.profile);
            patch.file_transcribe_selected_model_id = Some(Some(model.id.clone()));
            should_update = true;
        }
    }

    if should_update {
        let _ = update_settings_for_db_path(db_path, patch)?;
    }

    Ok(())
}

pub fn update_settings_for_db_path(db_path: &Path, patch: SettingsPatch) -> Result<AppSettings> {
    let current = get_settings_from_db_path(db_path)?;
    let next = AppSettings {
        default_mode: patch.default_mode.unwrap_or(current.default_mode),
        shortcut: patch.shortcut.unwrap_or(current.shortcut),
        shortcut_mode: patch.shortcut_mode.unwrap_or(current.shortcut_mode),
        language_mode: patch.language_mode.unwrap_or(current.language_mode),
        fixed_language: patch.fixed_language.unwrap_or(current.fixed_language),
        preferred_input_device: patch
            .preferred_input_device
            .unwrap_or(current.preferred_input_device),
        insert_behavior: patch.insert_behavior.unwrap_or(current.insert_behavior),
        launch_at_login_enabled: patch
            .launch_at_login_enabled
            .unwrap_or(current.launch_at_login_enabled),
        gpu_enabled: patch.gpu_enabled.unwrap_or(current.gpu_enabled),
        shortcut_dictation_model_profile: patch
            .shortcut_dictation_model_profile
            .unwrap_or(current.shortcut_dictation_model_profile),
        shortcut_dictation_selected_model_id: patch
            .shortcut_dictation_selected_model_id
            .unwrap_or(current.shortcut_dictation_selected_model_id),
        quick_dictate_model_profile: patch
            .quick_dictate_model_profile
            .unwrap_or(current.quick_dictate_model_profile),
        quick_dictate_selected_model_id: patch
            .quick_dictate_selected_model_id
            .unwrap_or(current.quick_dictate_selected_model_id),
        file_transcribe_model_profile: patch
            .file_transcribe_model_profile
            .unwrap_or(current.file_transcribe_model_profile),
        file_transcribe_selected_model_id: patch
            .file_transcribe_selected_model_id
            .unwrap_or(current.file_transcribe_selected_model_id),
        save_history: patch.save_history.unwrap_or(current.save_history),
        sounds_enabled: patch.sounds_enabled.unwrap_or(current.sounds_enabled),
        volume_ducking_enabled: patch
            .volume_ducking_enabled
            .unwrap_or(current.volume_ducking_enabled),
        file_diarization_enabled: patch
            .file_diarization_enabled
            .unwrap_or(current.file_diarization_enabled),
    };
    let connection = open_connection_by_path(db_path)?;
    connection.execute(
        "UPDATE settings SET default_mode = ?1, shortcut = ?2, shortcut_mode = ?3, language_mode = ?4, fixed_language = ?5, preferred_input_device = ?6, insert_behavior = ?7, launch_at_login_enabled = ?8, metal_enabled = ?9, shortcut_dictation_model_profile = ?10, shortcut_dictation_selected_model_id = ?11, quick_dictate_model_profile = ?12, quick_dictate_selected_model_id = ?13, file_transcribe_model_profile = ?14, file_transcribe_selected_model_id = ?15, save_history = ?16, sounds_enabled = ?17, volume_ducking_enabled = ?18, file_diarization_enabled = ?19 WHERE id = 1",
        params![
            to_default_mode(next.default_mode),
            next.shortcut,
            to_shortcut_mode(next.shortcut_mode),
            to_language_mode(next.language_mode),
            next.fixed_language,
            next.preferred_input_device,
            to_insert_behavior(next.insert_behavior),
            next.launch_at_login_enabled,
            next.gpu_enabled,
            to_model_profile(next.shortcut_dictation_model_profile),
            next.shortcut_dictation_selected_model_id,
            to_model_profile(next.quick_dictate_model_profile),
            next.quick_dictate_selected_model_id,
            to_model_profile(next.file_transcribe_model_profile),
            next.file_transcribe_selected_model_id,
            next.save_history,
            next.sounds_enabled,
            next.volume_ducking_enabled,
            next.file_diarization_enabled,
        ],
    )?;
    get_settings_from_db_path(db_path)
}

pub fn list_installed_models(state: &AppState) -> Result<Vec<InstalledModel>> {
    let connection = open_connection(state)?;
    let mut statement = connection.prepare(
        "SELECT id, engine, model_name, variant, local_path, size_bytes, is_default
         FROM installed_models ORDER BY model_name ASC",
    )?;
    let rows = statement.query_map([], map_installed_model_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read installed models")
}

pub fn get_settings(state: &AppState) -> Result<AppSettings> {
    let connection = open_connection(state)?;
    let settings = query_settings(&connection)?;
    Ok(settings)
}

pub fn get_settings_from_db_path(db_path: &Path) -> Result<AppSettings> {
    let connection = open_connection_by_path(db_path)?;
    let settings = query_settings(&connection)?;
    Ok(settings)
}

pub fn save_quick_dictation_transcript(
    db_path: &Path,
    result: &TranscriptResult,
    duration_ms: i64,
) -> Result<TranscriptSummary> {
    let connection = open_connection_by_path(db_path)?;
    let transaction = connection.unchecked_transaction()?;
    let transcript_id = Uuid::new_v4().to_string();
    insert_transcript(
        &transaction,
        &transcript_id,
        SourceType::QuickDictate,
        build_transcript_title(&result.plain_text),
        result,
        Some(duration_ms),
    )?;
    transaction.commit()?;
    fetch_transcript_summary(&connection, &transcript_id)
}

pub fn save_file_transcription(
    db_path: &Path,
    source_file: &SelectedSourceFile,
    result: &TranscriptResult,
) -> Result<TranscriptSummary> {
    let connection = open_connection_by_path(db_path)?;
    let transaction = connection.unchecked_transaction()?;
    let transcript_id = Uuid::new_v4().to_string();

    insert_transcript(
        &transaction,
        &transcript_id,
        SourceType::FileUpload,
        source_file.original_name.clone(),
        result,
        source_file.duration_ms,
    )?;

    transaction.execute(
        "INSERT INTO source_files (id, transcript_id, original_name, mime_type, local_path, duration_ms, size_bytes, sha256)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::new_v4().to_string(),
            &transcript_id,
            &source_file.original_name,
            &source_file.mime_type,
            &source_file.file_path,
            source_file.duration_ms,
            source_file.size_bytes,
            source_file.sha256.as_deref(),
        ],
    )?;

    transaction.commit()?;
    fetch_transcript_summary(&connection, &transcript_id)
}

pub fn get_file_transcription_performance(
    db_path: &Path,
    model_id: &str,
) -> Result<Option<FileTranscriptionPerformance>> {
    let connection = open_connection_by_path(db_path)?;
    let mut statement = connection.prepare(
        "SELECT model_id, avg_audio_ms_per_wall_ms, sample_count
         FROM file_transcription_model_performance
         WHERE model_id = ?1",
    )?;
    let mut rows = statement.query([model_id])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(FileTranscriptionPerformance {
            avg_audio_ms_per_wall_ms: row.get("avg_audio_ms_per_wall_ms")?,
            sample_count: row.get("sample_count")?,
        }));
    }
    Ok(None)
}

pub fn record_file_transcription_performance(
    db_path: &Path,
    model_id: &str,
    run_avg_audio_ms_per_wall_ms: f64,
) -> Result<()> {
    let connection = open_connection_by_path(db_path)?;
    let now = Utc::now().to_rfc3339();
    if let Some(existing) = get_file_transcription_performance(db_path, model_id)? {
        let next_count = existing.sample_count + 1;
        let next_avg = ((existing.avg_audio_ms_per_wall_ms * existing.sample_count as f64)
            + run_avg_audio_ms_per_wall_ms)
            / next_count as f64;
        connection.execute(
            "UPDATE file_transcription_model_performance
             SET avg_audio_ms_per_wall_ms = ?2, sample_count = ?3, updated_at = ?4
             WHERE model_id = ?1",
            params![model_id, next_avg, next_count, now],
        )?;
    } else {
        connection.execute(
            "INSERT INTO file_transcription_model_performance (
                model_id,
                avg_audio_ms_per_wall_ms,
                sample_count,
                updated_at
            ) VALUES (?1, ?2, 1, ?3)",
            params![model_id, run_avg_audio_ms_per_wall_ms, now],
        )?;
    }
    Ok(())
}

pub fn update_settings(state: &AppState, patch: SettingsPatch) -> Result<AppSettings> {
    update_settings_for_db_path(&state.db_path, patch)
}

pub fn list_transcripts(state: &AppState, query: Option<String>) -> Result<Vec<TranscriptSummary>> {
    let connection = open_connection(state)?;
    let normalized_query = query
        .as_ref()
        .map(|value| format!("%{}%", value.trim().to_lowercase()));
    let mut statement = if normalized_query.is_some() {
        connection.prepare(
            "SELECT id, created_at, source_type, title, plain_text, status, detected_languages, duration_ms, model_name, quality_status, recovered_region_count, diarization_status, speaker_count FROM transcripts WHERE lower(title) LIKE ?1 OR lower(plain_text) LIKE ?1 ORDER BY datetime(created_at) DESC",
        )?
    } else {
        connection.prepare(
            "SELECT id, created_at, source_type, title, plain_text, status, detected_languages, duration_ms, model_name, quality_status, recovered_region_count, diarization_status, speaker_count FROM transcripts ORDER BY datetime(created_at) DESC",
        )?
    };
    let rows = if let Some(value) = normalized_query {
        statement.query_map([value], map_transcript_summary_row)?
    } else {
        statement.query_map([], map_transcript_summary_row)?
    };
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read transcript list")
}

pub fn delete_transcript(state: &AppState, transcript_id: &str) -> Result<()> {
    let connection = open_connection(state)?;
    connection.execute(
        "DELETE FROM transcript_segments WHERE transcript_id = ?1",
        [transcript_id],
    )?;
    connection.execute(
        "DELETE FROM source_files WHERE transcript_id = ?1",
        [transcript_id],
    )?;
    connection.execute("DELETE FROM transcripts WHERE id = ?1", [transcript_id])?;
    Ok(())
}

pub fn delete_all_transcripts(state: &AppState) -> Result<()> {
    let connection = open_connection(state)?;
    connection.execute("DELETE FROM transcript_segments", [])?;
    connection.execute("DELETE FROM source_files", [])?;
    connection.execute("DELETE FROM transcripts", [])?;
    Ok(())
}

pub fn get_transcript(state: &AppState, transcript_id: &str) -> Result<TranscriptDetail> {
    let connection = open_connection(state)?;
    fetch_transcript_detail(&connection, transcript_id)
}

pub fn get_source_file(state: &AppState, transcript_id: &str) -> Result<SelectedSourceFile> {
    let connection = open_connection(state)?;
    connection.query_row(
        "SELECT local_path, original_name, mime_type, size_bytes, duration_ms, sha256 FROM source_files WHERE transcript_id = ?1",
        [transcript_id],
        |row| Ok(SelectedSourceFile {
            file_path: row.get(0)?, original_name: row.get(1)?, mime_type: row.get(2)?,
            size_bytes: row.get(3)?, duration_ms: row.get(4)?, sha256: row.get(5)?,
        }),
    ).context("SOURCE_FILE_REQUIRED: original source audio is unavailable")
}

pub fn replace_transcript_diarization(
    state: &AppState,
    transcript_id: &str,
    result: &TranscriptResult,
    replacement_source: Option<&SelectedSourceFile>,
) -> Result<TranscriptDetail> {
    let mut connection = open_connection(state)?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE transcripts SET diarization_status=?2, diarization_model_id=?3, diarization_warning=?4, diarization_policy_version=?5, speaker_count=?6, diarization_clustering_threshold=?7, diarization_speaker_count_hint=?8, diarization_source=?9 WHERE id=?1",
        params![transcript_id, to_diarization_status(result.diarization_status), &result.diarization_model_id,
            &result.diarization_warning, result.diarization_policy_version,
            if result.speakers.is_empty() { None } else { Some(result.speakers.len() as i32) },
            result.diarization_clustering_threshold, result.diarization_speaker_count_hint,
            to_diarization_source(result.diarization_source)],
    )?;
    for segment in &result.segments {
        transaction.execute(
            "UPDATE transcript_segments SET speaker_id=?2, speaker_ids_json=?3, speaker_attribution=?4, speaker_confidence=?5 WHERE id=?1 AND transcript_id=?6",
            params![&segment.id, &segment.speaker_id,
                segment.speaker_ids.as_ref().map(serde_json::to_string).transpose()?,
                to_speaker_attribution(segment.speaker_attribution), segment.speaker_confidence, transcript_id],
        )?;
    }
    transaction.execute(
        "DELETE FROM transcript_speakers WHERE transcript_id=?1",
        [transcript_id],
    )?;
    transaction.execute(
        "DELETE FROM diarization_turns WHERE transcript_id=?1",
        [transcript_id],
    )?;
    for speaker in &result.speakers {
        transaction.execute("INSERT INTO transcript_speakers (transcript_id,speaker_id,display_name,speaker_order) VALUES (?1,?2,?3,?4)",
            params![transcript_id, &speaker.speaker_id, &speaker.display_name, speaker.speaker_order])?;
    }
    for turn in &result.diarization_turns {
        transaction.execute("INSERT INTO diarization_turns (id,transcript_id,start_ms,end_ms,speaker_ids_json,confidence,is_overlap,is_uncertain,turn_order) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![format!("{transcript_id}:{}", turn.id), transcript_id, turn.start_ms, turn.end_ms,
                serde_json::to_string(&turn.speaker_ids)?, turn.confidence, turn.is_overlap, turn.is_uncertain, turn.turn_order])?;
    }
    if let Some(source) = replacement_source {
        transaction.execute("UPDATE source_files SET local_path=?2, original_name=?3, mime_type=?4, size_bytes=?5, duration_ms=?6, sha256=?7 WHERE transcript_id=?1",
            params![transcript_id, &source.file_path, &source.original_name, &source.mime_type, source.size_bytes, source.duration_ms, &source.sha256])?;
    }
    transaction.commit()?;
    fetch_transcript_detail(&connection, transcript_id)
}

pub fn rename_transcript(
    state: &AppState,
    transcript_id: &str,
    title: &str,
) -> Result<TranscriptSummary> {
    let title = validate_name(title, 200, "Transcript title")?;
    let connection = open_connection(state)?;
    let changed = connection.execute(
        "UPDATE transcripts SET title = ?2 WHERE id = ?1",
        params![transcript_id, title],
    )?;
    if changed == 0 {
        anyhow::bail!("Transcript not found.");
    }
    fetch_transcript_summary(&connection, transcript_id)
}

pub fn rename_transcript_speaker(
    state: &AppState,
    transcript_id: &str,
    speaker_id: &str,
    display_name: &str,
) -> Result<TranscriptDetail> {
    let display_name = validate_name(display_name, 80, "Speaker name")?;
    let connection = open_connection(state)?;
    let changed = connection.execute(
        "UPDATE transcript_speakers SET display_name = ?3 WHERE transcript_id = ?1 AND speaker_id = ?2",
        params![transcript_id, speaker_id, display_name],
    )?;
    if changed == 0 {
        anyhow::bail!("Speaker not found in transcript.");
    }
    fetch_transcript_detail(&connection, transcript_id)
}

fn open_connection(state: &AppState) -> Result<Connection> {
    open_connection_by_path(&state.db_path)
}

fn open_connection_by_path(db_path: &Path) -> Result<Connection> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open database at {}", db_path.display()))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}

fn query_settings(connection: &Connection) -> Result<AppSettings> {
    let settings = connection.query_row(
        "SELECT default_mode, shortcut, shortcut_mode, language_mode, fixed_language, preferred_input_device, insert_behavior, launch_at_login_enabled, metal_enabled, shortcut_dictation_model_profile, shortcut_dictation_selected_model_id, quick_dictate_model_profile, quick_dictate_selected_model_id, file_transcribe_model_profile, file_transcribe_selected_model_id, save_history, sounds_enabled, volume_ducking_enabled, file_diarization_enabled FROM settings WHERE id = 1",
        [],
        map_settings_row,
    )?;
    Ok(settings)
}

fn ensure_settings_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(settings)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut added_shortcut_dictation_columns = false;
    let mut added_quick_dictate_columns = false;
    let mut added_file_transcribe_columns = false;

    if !columns.iter().any(|column| column == "selected_model_id") {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN selected_model_id TEXT NULL",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "metal_enabled") {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN metal_enabled INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "preferred_input_device")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN preferred_input_device TEXT NULL",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "launch_at_login_enabled")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN launch_at_login_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "shortcut_dictation_model_profile")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN shortcut_dictation_model_profile TEXT NOT NULL DEFAULT 'balanced'",
            [],
        )?;
        added_shortcut_dictation_columns = true;
    }
    if !columns
        .iter()
        .any(|column| column == "shortcut_dictation_selected_model_id")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN shortcut_dictation_selected_model_id TEXT NULL",
            [],
        )?;
        added_shortcut_dictation_columns = true;
    }
    if !columns
        .iter()
        .any(|column| column == "quick_dictate_model_profile")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN quick_dictate_model_profile TEXT NOT NULL DEFAULT 'balanced'",
            [],
        )?;
        added_quick_dictate_columns = true;
    }
    if !columns
        .iter()
        .any(|column| column == "quick_dictate_selected_model_id")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN quick_dictate_selected_model_id TEXT NULL",
            [],
        )?;
        added_quick_dictate_columns = true;
    }
    if !columns
        .iter()
        .any(|column| column == "file_transcribe_model_profile")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN file_transcribe_model_profile TEXT NOT NULL DEFAULT 'balanced'",
            [],
        )?;
        added_file_transcribe_columns = true;
    }
    if !columns
        .iter()
        .any(|column| column == "file_transcribe_selected_model_id")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN file_transcribe_selected_model_id TEXT NULL",
            [],
        )?;
        added_file_transcribe_columns = true;
    }
    if !columns.iter().any(|column| column == "sounds_enabled") {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN sounds_enabled INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "volume_ducking_enabled")
    {
        connection.execute(
            "ALTER TABLE settings ADD COLUMN volume_ducking_enabled INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    for (name, declaration) in [
        ("file_diarization_enabled", "INTEGER NOT NULL DEFAULT 0"),
        (
            "quick_dictate_diarization_enabled",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        ("diarization_min_speakers", "INTEGER NULL"),
        ("diarization_max_speakers", "INTEGER NULL"),
        ("diarization_speaker_count", "INTEGER NULL"),
    ] {
        if !columns.iter().any(|column| column == name) {
            connection.execute(
                &format!("ALTER TABLE settings ADD COLUMN {name} {declaration}"),
                [],
            )?;
        }
    }
    connection.execute(
        "UPDATE settings SET diarization_speaker_count = diarization_min_speakers
         WHERE diarization_speaker_count IS NULL
           AND diarization_min_speakers IS NOT NULL
           AND diarization_min_speakers = diarization_max_speakers",
        [],
    )?;

    if added_quick_dictate_columns {
        connection.execute(
            "UPDATE settings
             SET
               quick_dictate_model_profile = COALESCE(model_profile, 'balanced'),
               quick_dictate_selected_model_id = selected_model_id
             WHERE id = 1",
            [],
        )?;
    }

    if added_shortcut_dictation_columns {
        connection.execute(
            "UPDATE settings
             SET
               shortcut_dictation_model_profile = COALESCE(quick_dictate_model_profile, model_profile, 'balanced'),
               shortcut_dictation_selected_model_id = COALESCE(quick_dictate_selected_model_id, selected_model_id)
             WHERE id = 1",
            [],
        )?;
    }

    if added_file_transcribe_columns {
        connection.execute(
            "UPDATE settings
             SET
               file_transcribe_model_profile = COALESCE(model_profile, 'balanced'),
               file_transcribe_selected_model_id = selected_model_id
             WHERE id = 1",
            [],
        )?;
    }

    Ok(())
}

fn ensure_vocabulary_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(custom_vocabulary_terms)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if !columns.iter().any(|column| column == "match_mode") {
        connection.execute(
            "ALTER TABLE custom_vocabulary_terms ADD COLUMN match_mode TEXT NOT NULL DEFAULT 'exact_and_fuzzy'",
            [],
        )?;
    }

    if columns.iter().any(|column| column == "category") {
        connection.execute(
            "ALTER TABLE custom_vocabulary_terms DROP COLUMN category",
            [],
        )?;
    }

    if columns.iter().any(|column| column == "language_hint") {
        connection.execute(
            "ALTER TABLE custom_vocabulary_terms DROP COLUMN language_hint",
            [],
        )?;
    }

    Ok(())
}

fn ensure_transcript_quality_columns(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(transcripts)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if !columns.iter().any(|column| column == "quality_status") {
        connection.execute(
            "ALTER TABLE transcripts ADD COLUMN quality_status TEXT NOT NULL DEFAULT 'clean'",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "recovered_region_count")
    {
        connection.execute(
            "ALTER TABLE transcripts ADD COLUMN recovered_region_count INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "transcription_warnings")
    {
        connection.execute(
            "ALTER TABLE transcripts ADD COLUMN transcription_warnings TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    Ok(())
}

/// Idempotently upgrades databases created by every prior Blabber release.
fn ensure_diarization_schema(connection: &Connection) -> Result<()> {
    let transcript_columns = table_columns(connection, "transcripts")?;
    for (name, declaration) in [
        (
            "diarization_status",
            "TEXT NOT NULL DEFAULT 'not_requested'",
        ),
        ("diarization_model_id", "TEXT NULL"),
        ("diarization_source", "TEXT NOT NULL DEFAULT 'none'"),
        ("diarization_warning", "TEXT NULL"),
        ("diarization_policy_version", "INTEGER NULL"),
        ("diarization_clustering_threshold", "REAL NULL"),
        ("diarization_speaker_count_hint", "INTEGER NULL"),
        ("speaker_count", "INTEGER NULL"),
    ] {
        if !transcript_columns.iter().any(|column| column == name) {
            connection.execute(
                &format!("ALTER TABLE transcripts ADD COLUMN {name} {declaration}"),
                [],
            )?;
        }
    }
    connection.execute(
        "UPDATE transcripts SET diarization_source='post_process' WHERE diarization_source='none' AND diarization_model_id=?1",
        [crate::diarization::DIARIZATION_MODEL_ID],
    )?;
    let segment_columns = table_columns(connection, "transcript_segments")?;
    for (name, declaration) in [
        ("speaker_id", "TEXT NULL"),
        ("speaker_ids_json", "TEXT NULL"),
        ("speaker_attribution", "TEXT NOT NULL DEFAULT 'none'"),
        ("speaker_confidence", "REAL NULL"),
    ] {
        if !segment_columns.iter().any(|column| column == name) {
            connection.execute(
                &format!("ALTER TABLE transcript_segments ADD COLUMN {name} {declaration}"),
                [],
            )?;
        }
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS transcript_speakers (
            transcript_id TEXT NOT NULL, speaker_id TEXT NOT NULL, display_name TEXT NOT NULL,
            speaker_order INTEGER NOT NULL, PRIMARY KEY (transcript_id, speaker_id),
            FOREIGN KEY (transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE);
         CREATE TABLE IF NOT EXISTS diarization_turns (
            id TEXT PRIMARY KEY, transcript_id TEXT NOT NULL, start_ms INTEGER NOT NULL,
            end_ms INTEGER NOT NULL, speaker_ids_json TEXT NOT NULL, confidence REAL NULL,
            is_overlap INTEGER NOT NULL DEFAULT 0, is_uncertain INTEGER NOT NULL DEFAULT 0,
            turn_order INTEGER NOT NULL,
            FOREIGN KEY (transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE);
         CREATE INDEX IF NOT EXISTS idx_diarization_turns_order ON diarization_turns(transcript_id, turn_order);
         CREATE INDEX IF NOT EXISTS idx_diarization_turns_start ON diarization_turns(transcript_id, start_ms);
         CREATE TABLE IF NOT EXISTS installed_model_packages (
            id TEXT PRIMARY KEY, capability TEXT NOT NULL, local_path TEXT NOT NULL,
            manifest_version INTEGER NOT NULL, hashes_json TEXT NOT NULL,
            installed_at TEXT NOT NULL);"
    )?;
    Ok(())
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>("name"))?;
    let columns = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns)
}

fn ensure_file_transcription_performance_table(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS file_transcription_model_performance (
            model_id TEXT PRIMARY KEY,
            avg_audio_ms_per_wall_ms REAL NOT NULL,
            sample_count INTEGER NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn seed_default_settings(connection: &Connection) -> Result<()> {
    connection.execute(
        "INSERT INTO settings (
            id,
            default_mode,
            shortcut,
            shortcut_mode,
            language_mode,
            fixed_language,
            preferred_input_device,
            insert_behavior,
            launch_at_login_enabled,
            metal_enabled,
            model_profile,
            selected_model_id,
            shortcut_dictation_model_profile,
            shortcut_dictation_selected_model_id,
            quick_dictate_model_profile,
            quick_dictate_selected_model_id,
            file_transcribe_model_profile,
            file_transcribe_selected_model_id,
            save_history,
            sounds_enabled,
            volume_ducking_enabled
        )
        SELECT
            1,
            'quick_dictate',
            'CmdOrCtrl+Shift+Space',
            'push_to_talk',
            'auto',
            NULL,
            NULL,
            'paste',
            0,
            1,
            'balanced',
            NULL,
            'balanced',
            NULL,
            'balanced',
            NULL,
            'balanced',
            NULL,
            1,
            1,
            1
        WHERE NOT EXISTS (SELECT 1 FROM settings WHERE id = 1)",
        [],
    )?;

    Ok(())
}

fn fetch_transcript_summary(
    connection: &Connection,
    transcript_id: &str,
) -> Result<TranscriptSummary> {
    let transcript = connection.query_row(
        "SELECT id, created_at, source_type, title, plain_text, status, detected_languages, duration_ms, model_name, quality_status, recovered_region_count, diarization_status, speaker_count FROM transcripts WHERE id = ?1",
        [transcript_id],
        map_transcript_summary_row,
    )?;
    Ok(transcript)
}

fn fetch_transcript_detail(
    connection: &Connection,
    transcript_id: &str,
) -> Result<TranscriptDetail> {
    let summary = fetch_transcript_summary(connection, transcript_id)?;
    let (full_text, timestamped_text, warnings_raw, diarization_model_id, diarization_source, diarization_warning, policy, clustering_threshold, speaker_count_hint): (
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<u32>,
        Option<f32>,
        Option<i32>,
    ) = connection.query_row(
        "SELECT full_text, timestamped_text, transcription_warnings, diarization_model_id, diarization_source, diarization_warning, diarization_policy_version, diarization_clustering_threshold, diarization_speaker_count_hint FROM transcripts WHERE id = ?1",
        [transcript_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
    )?;

    let mut segment_statement = connection.prepare(
        "SELECT id, start_ms, end_ms, text, COALESCE(language_code, 'und') AS language_code, segment_order, confidence, speaker_id, speaker_ids_json, speaker_attribution, speaker_confidence
         FROM transcript_segments WHERE transcript_id = ?1 ORDER BY segment_order ASC",
    )?;
    let segments = segment_statement
        .query_map([transcript_id], |row| {
            let speaker_ids_raw: Option<String> = row.get("speaker_ids_json")?;
            Ok(TranscriptSegment {
                id: row.get("id")?,
                start_ms: row.get("start_ms")?,
                end_ms: row.get("end_ms")?,
                text: row.get("text")?,
                language_code: row.get("language_code")?,
                segment_order: row.get("segment_order")?,
                confidence: row.get("confidence")?,
                speaker_id: row.get("speaker_id")?,
                speaker_ids: speaker_ids_raw.and_then(|value| serde_json::from_str(&value).ok()),
                speaker_attribution: parse_speaker_attribution(row.get("speaker_attribution")?)?,
                speaker_confidence: row.get("speaker_confidence")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut speaker_statement = connection.prepare(
        "SELECT speaker_id, display_name, speaker_order FROM transcript_speakers WHERE transcript_id = ?1 ORDER BY speaker_order ASC",
    )?;
    let speakers = speaker_statement
        .query_map([transcript_id], |row| {
            Ok(TranscriptSpeaker {
                speaker_id: row.get("speaker_id")?,
                display_name: row.get("display_name")?,
                speaker_order: row.get("speaker_order")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut turns_statement = connection.prepare(
        "SELECT id, start_ms, end_ms, speaker_ids_json, confidence, is_overlap, is_uncertain, turn_order
         FROM diarization_turns WHERE transcript_id = ?1 ORDER BY turn_order ASC",
    )?;
    let diarization_turns = turns_statement
        .query_map([transcript_id], |row| {
            let speaker_ids_raw: String = row.get("speaker_ids_json")?;
            Ok(DiarizationTurn {
                id: row.get("id")?,
                start_ms: row.get("start_ms")?,
                end_ms: row.get("end_ms")?,
                speaker_ids: serde_json::from_str(&speaker_ids_raw).unwrap_or_default(),
                confidence: row.get("confidence")?,
                is_overlap: row.get("is_overlap")?,
                is_uncertain: row.get("is_uncertain")?,
                turn_order: row.get("turn_order")?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(TranscriptDetail {
        summary,
        full_text,
        timestamped_text,
        transcription_warnings: serde_json::from_str(&warnings_raw).unwrap_or_default(),
        diarization_model_id,
        diarization_source: parse_diarization_source(diarization_source)?,
        diarization_warning,
        diarization_policy_version: policy,
        diarization_clustering_threshold: clustering_threshold,
        diarization_speaker_count_hint: speaker_count_hint,
        segments,
        speakers,
        diarization_turns,
    })
}

fn insert_transcript(
    transaction: &rusqlite::Transaction<'_>,
    transcript_id: &str,
    source_type: SourceType,
    title: String,
    result: &TranscriptResult,
    duration_ms: Option<i64>,
) -> Result<()> {
    let created_at = Utc::now().to_rfc3339();
    let languages = serde_json::to_string(&result.detected_languages)?;
    let warnings = serde_json::to_string(&result.warnings)?;
    transaction.execute(
        "INSERT INTO transcripts (id, created_at, source_type, title, full_text, plain_text, timestamped_text, detected_languages, duration_ms, status, model_name, quality_status, recovered_region_count, transcription_warnings, diarization_status, diarization_model_id, diarization_source, diarization_warning, diarization_policy_version, speaker_count, diarization_clustering_threshold, diarization_speaker_count_hint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        params![
            transcript_id,
            &created_at,
            to_source_type(source_type),
            &title,
            &result.full_text,
            &result.plain_text,
            &result.timestamped_text,
            languages,
            duration_ms,
            "completed",
            &result.model_name,
            to_transcript_quality_status(result.quality_status),
            result.recovered_region_count,
            warnings,
            to_diarization_status(result.diarization_status),
            &result.diarization_model_id,
            to_diarization_source(result.diarization_source),
            &result.diarization_warning,
            result.diarization_policy_version,
            if result.speakers.is_empty() { None } else { Some(result.speakers.len() as i32) },
            result.diarization_clustering_threshold,
            result.diarization_speaker_count_hint,
        ],
    )?;

    for segment in &result.segments {
        transaction.execute(
            "INSERT INTO transcript_segments (id, transcript_id, start_ms, end_ms, text, language_code, speaker_label, confidence, segment_order, speaker_id, speaker_ids_json, speaker_attribution, speaker_confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                &segment.id,
                transcript_id,
                segment.start_ms,
                segment.end_ms,
                &segment.text,
                &segment.language_code,
                segment.confidence,
                segment.segment_order,
                &segment.speaker_id,
                segment.speaker_ids.as_ref().map(serde_json::to_string).transpose()?,
                to_speaker_attribution(segment.speaker_attribution),
                segment.speaker_confidence,
            ],
        )?;
    }

    for speaker in &result.speakers {
        transaction.execute(
            "INSERT INTO transcript_speakers (transcript_id, speaker_id, display_name, speaker_order) VALUES (?1, ?2, ?3, ?4)",
            params![transcript_id, &speaker.speaker_id, &speaker.display_name, speaker.speaker_order],
        )?;
    }
    for turn in &result.diarization_turns {
        transaction.execute(
            "INSERT INTO diarization_turns (id, transcript_id, start_ms, end_ms, speaker_ids_json, confidence, is_overlap, is_uncertain, turn_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                format!("{transcript_id}:{}", turn.id), transcript_id, turn.start_ms, turn.end_ms,
                serde_json::to_string(&turn.speaker_ids)?, turn.confidence, turn.is_overlap,
                turn.is_uncertain, turn.turn_order
            ],
        )?;
    }

    Ok(())
}

fn build_transcript_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return format!("Quick dictate {}", Utc::now().format("%Y-%m-%d %H:%M:%S"));
    }

    let mut title = trimmed.chars().take(72).collect::<String>();
    if trimmed.chars().count() > 72 {
        title.push_str("...");
    }
    title
}

fn map_installed_model_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstalledModel> {
    let variant: String = row.get("variant")?;
    let profile = parse_model_profile(variant.clone())?;
    Ok(InstalledModel {
        id: row.get("id")?,
        engine: row.get("engine")?,
        model_name: row.get("model_name")?,
        variant,
        local_path: row.get("local_path")?,
        size_bytes: row.get("size_bytes")?,
        is_default: row.get("is_default")?,
        profile,
        capabilities: crate::model_metadata::capabilities_for_model(
            &row.get::<_, String>("id")?,
            &row.get::<_, String>("engine")?,
        ),
    })
}

fn map_settings_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppSettings> {
    Ok(AppSettings {
        default_mode: parse_default_mode(row.get("default_mode")?)?,
        shortcut: row.get("shortcut")?,
        shortcut_mode: parse_shortcut_mode(row.get("shortcut_mode")?)?,
        language_mode: parse_language_mode(row.get("language_mode")?)?,
        fixed_language: row.get("fixed_language")?,
        preferred_input_device: row.get("preferred_input_device")?,
        insert_behavior: parse_insert_behavior(row.get("insert_behavior")?)?,
        launch_at_login_enabled: row.get("launch_at_login_enabled")?,
        gpu_enabled: row.get("metal_enabled")?,
        shortcut_dictation_model_profile: parse_model_profile(
            row.get("shortcut_dictation_model_profile")?,
        )?,
        shortcut_dictation_selected_model_id: row.get("shortcut_dictation_selected_model_id")?,
        quick_dictate_model_profile: parse_model_profile(row.get("quick_dictate_model_profile")?)?,
        quick_dictate_selected_model_id: row.get("quick_dictate_selected_model_id")?,
        file_transcribe_model_profile: parse_model_profile(
            row.get("file_transcribe_model_profile")?,
        )?,
        file_transcribe_selected_model_id: row.get("file_transcribe_selected_model_id")?,
        save_history: row.get("save_history")?,
        sounds_enabled: row.get("sounds_enabled")?,
        volume_ducking_enabled: row.get("volume_ducking_enabled")?,
        file_diarization_enabled: row.get("file_diarization_enabled")?,
    })
}

#[cfg(target_os = "linux")]
fn shortcut_model_preferences() -> &'static [&'static str] {
    &["ggml-small.bin", "ggml-small.en.bin", "ggml-base.bin"]
}
#[cfg(not(target_os = "linux"))]
fn shortcut_model_preferences() -> &'static [&'static str] {
    &["ggml-medium.bin"]
}

#[cfg(target_os = "linux")]
fn quick_dictate_model_preferences() -> &'static [&'static str] {
    &["ggml-small.bin", "ggml-small.en.bin", "ggml-base.bin"]
}
#[cfg(not(target_os = "linux"))]
fn quick_dictate_model_preferences() -> &'static [&'static str] {
    &["ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3-turbo.bin"]
}

#[cfg(target_os = "linux")]
fn file_transcribe_model_preferences() -> &'static [&'static str] {
    &["ggml-small.bin", "ggml-base.bin"]
}
#[cfg(not(target_os = "linux"))]
fn file_transcribe_model_preferences() -> &'static [&'static str] {
    &["ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3-turbo.bin"]
}

#[cfg(target_os = "linux")]
fn fallback_shortcut_profile() -> ModelProfile {
    ModelProfile::Balanced
}
#[cfg(not(target_os = "linux"))]
fn fallback_shortcut_profile() -> ModelProfile {
    ModelProfile::Balanced
}

#[cfg(target_os = "linux")]
fn fallback_quick_dictate_profile() -> ModelProfile {
    ModelProfile::Balanced
}
#[cfg(not(target_os = "linux"))]
fn fallback_quick_dictate_profile() -> ModelProfile {
    ModelProfile::Accurate
}

#[cfg(target_os = "linux")]
fn fallback_file_transcribe_profile() -> ModelProfile {
    ModelProfile::Balanced
}
#[cfg(not(target_os = "linux"))]
fn fallback_file_transcribe_profile() -> ModelProfile {
    ModelProfile::Accurate
}

fn find_model_by_name(
    models: &[InstalledModel],
    preferred_names: &[&str],
) -> Option<InstalledModel> {
    preferred_names.iter().find_map(|name| {
        models
            .iter()
            .find(|model| model.model_name == *name)
            .cloned()
    })
}

fn find_model_by_id(models: &[InstalledModel], model_id: &str) -> Option<InstalledModel> {
    models.iter().find(|model| model.id == model_id).cloned()
}

fn resolve_profile_model(
    models: &[InstalledModel],
    profile: ModelProfile,
) -> Option<InstalledModel> {
    models
        .iter()
        .find(|model| model.profile == profile && model.is_default)
        .or_else(|| models.iter().find(|model| model.profile == profile))
        .cloned()
}

fn map_transcript_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptSummary> {
    let languages_raw: String = row.get("detected_languages")?;
    let detected_languages = serde_json::from_str(&languages_raw).unwrap_or_default();
    Ok(TranscriptSummary {
        id: row.get("id")?,
        created_at: row.get("created_at")?,
        source_type: parse_source_type(row.get("source_type")?)?,
        title: row.get("title")?,
        plain_text: row.get("plain_text")?,
        status: parse_transcript_status(row.get("status")?)?,
        detected_languages,
        duration_ms: row.get("duration_ms")?,
        model_name: row.get("model_name")?,
        quality_status: parse_transcript_quality_status(row.get("quality_status")?)?,
        recovered_region_count: row.get("recovered_region_count")?,
        diarization_status: parse_diarization_status(row.get("diarization_status")?)?,
        speaker_count: row.get("speaker_count")?,
    })
}

fn parse_diarization_status(value: String) -> rusqlite::Result<DiarizationStatus> {
    match value.as_str() {
        "not_requested" => Ok(DiarizationStatus::NotRequested),
        "pending" => Ok(DiarizationStatus::Pending),
        "running" => Ok(DiarizationStatus::Running),
        "completed" => Ok(DiarizationStatus::Completed),
        "completed_with_uncertainty" => Ok(DiarizationStatus::CompletedWithUncertainty),
        "failed" => Ok(DiarizationStatus::Failed),
        "canceled" => Ok(DiarizationStatus::Canceled),
        "not_enough_speech" => Ok(DiarizationStatus::NotEnoughSpeech),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn to_diarization_status(value: DiarizationStatus) -> &'static str {
    match value {
        DiarizationStatus::NotRequested => "not_requested",
        DiarizationStatus::Pending => "pending",
        DiarizationStatus::Running => "running",
        DiarizationStatus::Completed => "completed",
        DiarizationStatus::CompletedWithUncertainty => "completed_with_uncertainty",
        DiarizationStatus::Failed => "failed",
        DiarizationStatus::Canceled => "canceled",
        DiarizationStatus::NotEnoughSpeech => "not_enough_speech",
    }
}

fn parse_diarization_source(value: String) -> rusqlite::Result<DiarizationSource> {
    match value.as_str() {
        "none" => Ok(DiarizationSource::None),
        "native_model" => Ok(DiarizationSource::NativeModel),
        "post_process" => Ok(DiarizationSource::PostProcess),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn to_diarization_source(value: DiarizationSource) -> &'static str {
    match value {
        DiarizationSource::None => "none",
        DiarizationSource::NativeModel => "native_model",
        DiarizationSource::PostProcess => "post_process",
    }
}

fn parse_speaker_attribution(value: String) -> rusqlite::Result<SpeakerAttribution> {
    match value.as_str() {
        "none" => Ok(SpeakerAttribution::None),
        "assigned" => Ok(SpeakerAttribution::Assigned),
        "uncertain" => Ok(SpeakerAttribution::Uncertain),
        "likely" => Ok(SpeakerAttribution::Likely),
        "overlap" => Ok(SpeakerAttribution::Overlap),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn to_speaker_attribution(value: SpeakerAttribution) -> &'static str {
    match value {
        SpeakerAttribution::None => "none",
        SpeakerAttribution::Assigned => "assigned",
        SpeakerAttribution::Uncertain => "uncertain",
        SpeakerAttribution::Likely => "likely",
        SpeakerAttribution::Overlap => "overlap",
    }
}

fn validate_name<'a>(value: &'a str, max_chars: usize, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("{label} cannot be empty.");
    }
    if value.chars().count() > max_chars {
        anyhow::bail!("{label} cannot exceed {max_chars} characters.");
    }
    Ok(value)
}

fn parse_transcript_quality_status(value: String) -> rusqlite::Result<TranscriptQualityStatus> {
    match value.as_str() {
        "clean" => Ok(TranscriptQualityStatus::Clean),
        "recovered" => Ok(TranscriptQualityStatus::Recovered),
        "partial" => Ok(TranscriptQualityStatus::Partial),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn to_transcript_quality_status(value: TranscriptQualityStatus) -> &'static str {
    match value {
        TranscriptQualityStatus::Clean => "clean",
        TranscriptQualityStatus::Recovered => "recovered",
        TranscriptQualityStatus::Partial => "partial",
    }
}

fn parse_default_mode(value: String) -> rusqlite::Result<DefaultMode> {
    match value.as_str() {
        "quick_dictate" => Ok(DefaultMode::QuickDictate),
        "file_transcribe" => Ok(DefaultMode::FileTranscribe),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_shortcut_mode(value: String) -> rusqlite::Result<ShortcutMode> {
    match value.as_str() {
        "push_to_talk" => Ok(ShortcutMode::PushToTalk),
        "toggle" => Ok(ShortcutMode::Toggle),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_language_mode(value: String) -> rusqlite::Result<LanguageMode> {
    match value.as_str() {
        "auto" => Ok(LanguageMode::Auto),
        "fixed" => Ok(LanguageMode::Fixed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_insert_behavior(value: String) -> rusqlite::Result<InsertBehavior> {
    match value.as_str() {
        "paste" => Ok(InsertBehavior::Paste),
        "clipboard_only" => Ok(InsertBehavior::ClipboardOnly),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_model_profile(value: String) -> rusqlite::Result<ModelProfile> {
    match value.as_str() {
        "fast" => Ok(ModelProfile::Fast),
        "balanced" => Ok(ModelProfile::Balanced),
        "accurate" | "1.7B BF16" | "0.9B F16" | "8-bit MLX" => Ok(ModelProfile::Accurate),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_source_type(value: String) -> rusqlite::Result<SourceType> {
    match value.as_str() {
        "quick_dictate" => Ok(SourceType::QuickDictate),
        "file_upload" => Ok(SourceType::FileUpload),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_transcript_status(value: String) -> rusqlite::Result<TranscriptStatus> {
    match value.as_str() {
        "queued" => Ok(TranscriptStatus::Queued),
        "recording" => Ok(TranscriptStatus::Recording),
        "processing" => Ok(TranscriptStatus::Processing),
        "completed" => Ok(TranscriptStatus::Completed),
        "failed" => Ok(TranscriptStatus::Failed),
        "canceled" => Ok(TranscriptStatus::Canceled),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn to_default_mode(value: DefaultMode) -> &'static str {
    match value {
        DefaultMode::QuickDictate => "quick_dictate",
        DefaultMode::FileTranscribe => "file_transcribe",
    }
}

fn to_shortcut_mode(value: ShortcutMode) -> &'static str {
    match value {
        ShortcutMode::PushToTalk => "push_to_talk",
        ShortcutMode::Toggle => "toggle",
    }
}

fn to_language_mode(value: LanguageMode) -> &'static str {
    match value {
        LanguageMode::Auto => "auto",
        LanguageMode::Fixed => "fixed",
    }
}

fn to_insert_behavior(value: InsertBehavior) -> &'static str {
    match value {
        InsertBehavior::Paste => "paste",
        InsertBehavior::ClipboardOnly => "clipboard_only",
    }
}

fn to_model_profile(value: ModelProfile) -> &'static str {
    match value {
        ModelProfile::Fast => "fast",
        ModelProfile::Balanced => "balanced",
        ModelProfile::Accurate => "accurate",
    }
}

fn to_source_type(value: SourceType) -> &'static str {
    match value {
        SourceType::QuickDictate => "quick_dictate",
        SourceType::FileUpload => "file_upload",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_model_variants_map_to_accurate_profile() {
        for variant in ["1.7B BF16", "0.9B F16", "8-bit MLX"] {
            assert_eq!(
                parse_model_profile(variant.to_string()).expect("known native model variant"),
                ModelProfile::Accurate,
            );
        }
    }

    #[test]
    fn obsolete_vocabulary_columns_are_removed_without_losing_terms_or_aliases() {
        let connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch(
                "CREATE TABLE custom_vocabulary_terms (
                    id TEXT PRIMARY KEY,
                    canonical TEXT NOT NULL,
                    normalized_canonical TEXT NOT NULL UNIQUE,
                    category TEXT NOT NULL,
                    language_hint TEXT NULL,
                    match_mode TEXT NOT NULL DEFAULT 'exact_and_fuzzy',
                    is_builtin INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE custom_vocabulary_aliases (
                    id TEXT PRIMARY KEY,
                    term_id TEXT NOT NULL,
                    alias TEXT NOT NULL,
                    normalized_alias TEXT NOT NULL UNIQUE,
                    FOREIGN KEY (term_id) REFERENCES custom_vocabulary_terms(id) ON DELETE CASCADE
                );
                INSERT INTO custom_vocabulary_terms
                    (id, canonical, normalized_canonical, category, language_hint, match_mode, is_builtin, created_at, updated_at)
                VALUES
                    ('legacy', 'CloudOpus', 'cloudopus', 'brand', 'en', 'exact_only', 0, 'created', 'updated');
                INSERT INTO custom_vocabulary_aliases
                    (id, term_id, alias, normalized_alias)
                VALUES
                    ('legacy-alias', 'legacy', 'cloud opus', 'cloud opus');",
            )
            .expect("legacy vocabulary schema");

        ensure_vocabulary_columns(&connection).expect("vocabulary migration");

        let mut statement = connection
            .prepare("PRAGMA table_info(custom_vocabulary_terms)")
            .expect("table info");
        let columns = statement
            .query_map([], |row| row.get::<_, String>("name"))
            .expect("columns")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("column names");
        assert!(!columns.iter().any(|column| column == "category"));
        assert!(!columns.iter().any(|column| column == "language_hint"));

        let term = connection
            .query_row(
                "SELECT canonical, match_mode, created_at, updated_at FROM custom_vocabulary_terms WHERE id = 'legacy'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .expect("migrated term");
        assert_eq!(
            term,
            (
                "CloudOpus".to_string(),
                "exact_only".to_string(),
                "created".to_string(),
                "updated".to_string(),
            )
        );
        let alias_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM custom_vocabulary_aliases WHERE term_id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .expect("preserved alias");
        assert_eq!(alias_count, 1);
    }

    #[test]
    fn quality_columns_are_added_to_existing_transcript_tables() {
        let connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .execute_batch("CREATE TABLE transcripts (id TEXT PRIMARY KEY);")
            .expect("legacy transcript table");

        ensure_transcript_quality_columns(&connection).expect("quality migration");
        connection
            .execute("INSERT INTO transcripts (id) VALUES ('legacy')", [])
            .expect("legacy row");

        let values = connection
            .query_row(
                "SELECT quality_status, recovered_region_count, transcription_warnings FROM transcripts WHERE id = 'legacy'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("quality defaults");
        assert_eq!(values, ("clean".to_string(), 0, "[]".to_string()));
    }

    #[test]
    fn speaker_metadata_roundtrips_atomically() {
        let connection = Connection::open_in_memory().expect("in-memory database");
        connection.execute_batch(INIT_MIGRATION).expect("schema");
        let transaction = connection.unchecked_transaction().expect("transaction");
        let result = TranscriptResult {
            job_id: "job".into(),
            model_name: "test".into(),
            full_text: "Hello".into(),
            plain_text: "Hello".into(),
            timestamped_text: "[00:00 - 00:01] en: Hello".into(),
            detected_languages: vec!["en".into()],
            segments: vec![TranscriptSegment {
                id: "segment".into(),
                start_ms: 0,
                end_ms: 1_000,
                text: "Hello".into(),
                language_code: "en".into(),
                segment_order: 0,
                confidence: Some(0.9),
                speaker_id: Some("speaker_0".into()),
                speaker_ids: Some(vec!["speaker_0".into()]),
                speaker_attribution: SpeakerAttribution::Assigned,
                speaker_confidence: Some(1.0),
            }],
            quality_status: TranscriptQualityStatus::Clean,
            recovered_region_count: 0,
            warnings: vec![],
            diarization_status: DiarizationStatus::Completed,
            diarization_model_id: Some(crate::diarization::DIARIZATION_MODEL_ID.into()),
            diarization_source: DiarizationSource::PostProcess,
            diarization_warning: None,
            diarization_policy_version: Some(1),
            diarization_clustering_threshold: Some(1.1),
            diarization_speaker_count_hint: None,
            speakers: vec![TranscriptSpeaker {
                speaker_id: "speaker_0".into(),
                display_name: "Speaker 1".into(),
                speaker_order: 0,
            }],
            diarization_turns: vec![DiarizationTurn {
                id: "turn_0".into(),
                start_ms: 0,
                end_ms: 1_000,
                speaker_ids: vec!["speaker_0".into()],
                confidence: None,
                is_overlap: false,
                is_uncertain: false,
                turn_order: 0,
            }],
        };
        insert_transcript(
            &transaction,
            "transcript",
            SourceType::FileUpload,
            "Test".into(),
            &result,
            Some(1_000),
        )
        .expect("insert transcript");
        transaction.commit().expect("commit");

        let detail = fetch_transcript_detail(&connection, "transcript").expect("detail");
        assert_eq!(detail.summary.speaker_count, Some(1));
        assert_eq!(detail.segments[0].speaker_id.as_deref(), Some("speaker_0"));
        assert_eq!(detail.speakers[0].display_name, "Speaker 1");
        assert_eq!(detail.diarization_turns.len(), 1);
        assert_eq!(detail.diarization_source, DiarizationSource::PostProcess);
    }

    #[test]
    fn tiny_retirement_deletes_only_direct_managed_weights_and_partials() {
        let root = std::env::temp_dir().join(format!("blabber-tiny-retirement-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("directories");
        fs::write(root.join("ggml-tiny.bin"), b"weight").expect("weight");
        fs::write(root.join("ggml-tiny.en.bin.part"), b"partial").expect("partial");
        fs::write(root.join("ggml-small.bin"), b"keep").expect("small");
        fs::write(root.join("nested/ggml-tiny.bin"), b"keep nested").expect("nested");

        retire_whisper_tiny_files(&root).expect("retirement");
        assert!(!root.join("ggml-tiny.bin").exists());
        assert!(!root.join("ggml-tiny.en.bin.part").exists());
        assert!(root.join("ggml-small.bin").exists());
        assert!(root.join("nested/ggml-tiny.bin").exists());
        assert!(is_tiny_selection(Some("ggml-tiny-bin")));
        assert!(!is_tiny_selection(Some("ggml-small-bin")));

        fs::remove_dir_all(root).expect("cleanup");
    }
}
