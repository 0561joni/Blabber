use serde::{Deserialize, Serialize};

pub const MOSS_MODEL_ID: &str = "moss-transcribe-diarize-0.9b-f16";
pub const MOSS_MODEL_NAME: &str = "MOSS Transcribe + Diarize 0.9B F16";
pub const MOSS_MODEL_DIR: &str = "moss-transcribe-diarize-0.9b-f16";
pub const MOSS_MODEL_REVISION: &str = "54e4bbd17da3f84adf1c1bcf7791b9b9266f741e";
pub const MOSS_MODEL_SIZE: i64 = 1_833_647_104;

pub const VIBEVOICE_MODEL_ID: &str = "vibevoice-asr-8bit-mlx";
pub const VIBEVOICE_MODEL_NAME: &str = "VibeVoice-ASR 8-bit MLX";
pub const VIBEVOICE_MODEL_DIR: &str = "vibevoice-asr-8bit-mlx";
pub const VIBEVOICE_MODEL_REVISION: &str = "725c72e54d6ef875472c27fbc50fab470a960940";
pub const VIBEVOICE_MODEL_SIZE: i64 = 9_521_624_407;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelUseContext {
    ShortcutDictation,
    QuickDictate,
    FileTranscription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLanguageControl {
    AutomaticAndFixed,
    AutomaticOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub supported_contexts: Vec<ModelUseContext>,
    pub native_diarization: bool,
    pub timestamped_segments: bool,
    pub context_support: bool,
    pub language_control: ModelLanguageControl,
    pub maximum_audio_duration_ms: Option<i64>,
}

impl ModelCapabilities {
    pub fn standard_asr() -> Self {
        Self {
            supported_contexts: vec![
                ModelUseContext::ShortcutDictation,
                ModelUseContext::QuickDictate,
                ModelUseContext::FileTranscription,
            ],
            native_diarization: false,
            timestamped_segments: true,
            context_support: true,
            language_control: ModelLanguageControl::AutomaticAndFixed,
            maximum_audio_duration_ms: None,
        }
    }

    pub fn moss() -> Self {
        Self {
            native_diarization: true,
            language_control: ModelLanguageControl::AutomaticOnly,
            maximum_audio_duration_ms: Some(90 * 60 * 1_000),
            ..Self::standard_asr()
        }
    }

    pub fn vibevoice() -> Self {
        Self {
            supported_contexts: vec![ModelUseContext::FileTranscription],
            native_diarization: true,
            timestamped_segments: true,
            context_support: true,
            language_control: ModelLanguageControl::AutomaticOnly,
            maximum_audio_duration_ms: Some(60 * 60 * 1_000),
        }
    }

    pub fn package_only() -> Self {
        Self {
            supported_contexts: Vec::new(),
            native_diarization: false,
            timestamped_segments: false,
            context_support: false,
            language_control: ModelLanguageControl::AutomaticOnly,
            maximum_audio_duration_ms: None,
        }
    }
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self::standard_asr()
    }
}

pub fn capabilities_for_model(model_id: &str, engine: &str) -> ModelCapabilities {
    match model_id {
        MOSS_MODEL_ID => ModelCapabilities::moss(),
        VIBEVOICE_MODEL_ID => ModelCapabilities::vibevoice(),
        _ if engine == "sherpa-onnx" || engine == "whisper.cpp-vad" => {
            ModelCapabilities::package_only()
        }
        _ => ModelCapabilities::standard_asr(),
    }
}

pub fn vibevoice_platform_supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64")) && macos_major_version() >= Some(14)
}

#[cfg(target_os = "macos")]
fn macos_major_version() -> Option<u32> {
    std::process::Command::new("sw_vers")
        .args(["-productVersion"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|version| version.trim().split('.').next()?.parse().ok())
}

#[cfg(not(target_os = "macos"))]
fn macos_major_version() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_models_publish_their_context_and_duration_limits() {
        let moss = capabilities_for_model(MOSS_MODEL_ID, "moss-transcribe-cpp");
        assert_eq!(moss.supported_contexts.len(), 3);
        assert!(moss.native_diarization);
        assert_eq!(moss.maximum_audio_duration_ms, Some(5_400_000));

        let vibe = capabilities_for_model(VIBEVOICE_MODEL_ID, "vibevoice-mlx");
        assert_eq!(
            vibe.supported_contexts,
            [ModelUseContext::FileTranscription]
        );
        assert_eq!(vibe.maximum_audio_duration_ms, Some(3_600_000));
    }
}
