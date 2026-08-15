use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResponse {
    pub app_name: String,
    pub app_version: String,
    pub platform: String,
    pub db_path: String,
    pub temp_dir: String,
    pub models_dir: String,
    pub startup_notices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutMode {
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageMode {
    Auto,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertBehavior {
    Paste,
    ClipboardOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfile {
    Fast,
    Balanced,
    Accurate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMode {
    QuickDictate,
    FileTranscribe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_mode: DefaultMode,
    pub shortcut: String,
    pub shortcut_mode: ShortcutMode,
    pub language_mode: LanguageMode,
    pub fixed_language: Option<String>,
    pub preferred_input_device: Option<String>,
    pub insert_behavior: InsertBehavior,
    pub launch_at_login_enabled: bool,
    pub gpu_enabled: bool,
    pub shortcut_dictation_model_profile: ModelProfile,
    pub shortcut_dictation_selected_model_id: Option<String>,
    pub quick_dictate_model_profile: ModelProfile,
    pub quick_dictate_selected_model_id: Option<String>,
    pub file_transcribe_model_profile: ModelProfile,
    pub file_transcribe_selected_model_id: Option<String>,
    pub save_history: bool,
    pub sounds_enabled: bool,
    pub volume_ducking_enabled: bool,
    pub file_diarization_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub default_mode: Option<DefaultMode>,
    pub shortcut: Option<String>,
    pub shortcut_mode: Option<ShortcutMode>,
    pub language_mode: Option<LanguageMode>,
    pub fixed_language: Option<Option<String>>,
    pub preferred_input_device: Option<Option<String>>,
    pub insert_behavior: Option<InsertBehavior>,
    pub launch_at_login_enabled: Option<bool>,
    pub gpu_enabled: Option<bool>,
    pub shortcut_dictation_model_profile: Option<ModelProfile>,
    pub shortcut_dictation_selected_model_id: Option<Option<String>>,
    pub quick_dictate_model_profile: Option<ModelProfile>,
    pub quick_dictate_selected_model_id: Option<Option<String>>,
    pub file_transcribe_model_profile: Option<ModelProfile>,
    pub file_transcribe_selected_model_id: Option<Option<String>>,
    pub save_history: Option<bool>,
    pub sounds_enabled: Option<bool>,
    pub volume_ducking_enabled: Option<bool>,
    pub file_diarization_enabled: Option<bool>,
}
