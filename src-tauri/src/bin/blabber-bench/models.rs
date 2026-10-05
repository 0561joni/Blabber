//! The engines under test, discovered the way the app discovers them.
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use speech_to_text_lib::{asr, live_pair, model_downloads, native_asr, r2t2};

use crate::paths::Paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Engine {
    Batch {
        installed: asr::InstalledModel,
    },
    R2t2 {
        helper: PathBuf,
        model: PathBuf,
    },
    LivePair {
        helper: PathBuf,
        nemotron: PathBuf,
        parakeet: PathBuf,
        chunk_ms: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchModel {
    pub id: String,
    pub app_model_id: String,
    pub name: String,
    pub engine_name: String,
    pub size_bytes: Option<u64>,
    pub engine: Engine,
}

impl BenchModel {
    pub fn streaming(&self) -> bool {
        !matches!(self.engine, Engine::Batch { .. })
    }

    /// File-only diarising models run on long clips unless forced.
    pub fn long_only(&self) -> bool {
        matches!(self.engine_name.as_str(), "moss-transcribe-cpp" | "vibevoice-mlx")
    }

    pub fn max_duration_ms(&self) -> Option<u64> {
        match &self.engine {
            Engine::Batch { installed } => installed
                .capabilities
                .maximum_audio_duration_ms
                .map(|ms| ms as u64),
            _ => Some(300_000),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Unavailable {
    pub id: String,
    pub reason: String,
}

fn bench_id(installed: &asr::InstalledModel) -> String {
    match installed.id.as_str() {
        "ggml-small-bin" => "whisper-small".into(),
        "ggml-medium-bin" => "whisper-medium".into(),
        "ggml-large-v3-bin" => "whisper-large-v3".into(),
        "ggml-large-v3-turbo-bin" => "whisper-turbo".into(),
        "ggml-large-v3-turbo-q5_0-bin" => "whisper-turbo-q5".into(),
        speech_to_text_lib::qwen_asr::QWEN_MODEL_ID => "qwen3-asr".into(),
        id if id.starts_with("moss-") => "moss".into(),
        id if id.starts_with("vibevoice-") => "vibevoice".into(),
        id => {
            let stem = id.trim_start_matches("ggml-").trim_end_matches("-bin");
            if installed.engine == "whisper.cpp" {
                format!("whisper-{stem}")
            } else {
                stem.into()
            }
        }
    }
}

/// Release builds only look for the MOSS and VibeVoice workers next to the
/// executable or in their environment variables; point those at the built
/// workers so discovery and the children find them. Call before any thread starts.
pub fn export_worker_env(paths: &Paths) {
    for (key, directory, name) in [
        ("BLABBER_MOSS_WORKER", "moss", "blabber-moss-worker"),
        ("BLABBER_VIBEVOICE_WORKER", "vibevoice", "blabber-vibevoice-worker"),
    ] {
        if std::env::var_os(key).is_some() {
            continue;
        }
        let found = [
            paths.manifest_dir.join("bundle").join(directory).join(name).join(name),
            PathBuf::from("/Applications/Blabber.app/Contents/Resources/workers")
                .join(directory)
                .join(name)
                .join(name),
        ]
        .into_iter()
        .find(|path| path.is_file());
        if let Some(path) = found {
            std::env::set_var(key, path);
        }
    }
}

fn dir_size(path: &Path) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        return Some(metadata.len());
    }
    let mut total = 0;
    for entry in std::fs::read_dir(path).ok()? {
        total += dir_size(&entry.ok()?.path()).unwrap_or(0);
    }
    Some(total)
}

/// Every engine Blabber can run on this Mac, plus the reasons others cannot.
pub fn discover(paths: &Paths, live_pair_chunk_ms: u32) -> Result<(Vec<BenchModel>, Vec<Unavailable>)> {
    let mut models = Vec::new();
    let mut unavailable = Vec::new();
    for installed in asr::discover_installed_models(&paths.models)? {
        // R2T2 and the live pair are listed too; they get their own adapters below.
        if installed.capabilities.streaming_transcription {
            continue;
        }
        let id = bench_id(&installed);
        let native = matches!(installed.engine.as_str(), "moss-transcribe-cpp" | "vibevoice-mlx");
        if native && !native_asr::worker_available(&installed.id) {
            unavailable.push(Unavailable {
                id,
                reason: format!("{} worker not found (build it or set its BLABBER_*_WORKER variable)", installed.model_name),
            });
            continue;
        }
        models.push(BenchModel {
            id,
            app_model_id: installed.id.clone(),
            name: installed.model_name.clone(),
            engine_name: installed.engine.clone(),
            size_bytes: Some(installed.size_bytes.max(0) as u64),
            engine: Engine::Batch { installed },
        });
    }

    let r2t2_model = [
        r2t2::model_path(&paths.models),
        paths.manifest_dir.join("target/r2t2-model").join(r2t2::MODEL_FILE),
    ]
    .into_iter()
    .find(|path| std::fs::metadata(path).map(|m| m.len()).ok() == Some(r2t2::MODEL_BYTES as u64));
    match (
        paths.helper("r2t2/blabber-r2t2-worker", "workers/r2t2/blabber-r2t2-worker"),
        r2t2_model,
    ) {
        (Some(helper), Some(model)) if r2t2::platform_supported() => models.push(BenchModel {
            id: "r2t2".into(),
            app_model_id: r2t2::MODEL_ID.into(),
            name: r2t2::MODEL_NAME.into(),
            engine_name: "audio.cpp-r2t2".into(),
            size_bytes: Some(r2t2::MODEL_BYTES as u64),
            engine: Engine::R2t2 { helper, model },
        }),
        (helper, _) => unavailable.push(Unavailable {
            id: "r2t2".into(),
            reason: if !r2t2::platform_supported() {
                "requires Apple Silicon and macOS 14".into()
            } else if helper.is_none() {
                "helper missing (npm run build:r2t2)".into()
            } else {
                "model not downloaded".into()
            },
        }),
    }

    let installed_root = paths.models.join(live_pair::MODEL_ID);
    let pair_root = if model_downloads::live_pair_installed(&paths.models) {
        Some(installed_root)
    } else {
        let development = paths.manifest_dir.join("target/fluid-models");
        development.join("parakeet").is_dir().then_some(development)
    };
    let chunk_ms = if [560, 1120].contains(&live_pair_chunk_ms) { live_pair_chunk_ms } else { 560 };
    match (
        paths.helper("fluid/blabber-fluid-worker", "workers/fluid/blabber-fluid-worker"),
        pair_root,
    ) {
        (Some(helper), Some(root)) if live_pair::platform_supported() => {
            let nemotron = root.join("nemotron/latin").join(format!("{chunk_ms}ms"));
            let parakeet = root.join("parakeet");
            if nemotron.is_dir() && parakeet.is_dir() {
                models.push(BenchModel {
                    id: "live-pair".into(),
                    app_model_id: live_pair::MODEL_ID.into(),
                    name: live_pair::MODEL_NAME.into(),
                    engine_name: live_pair::ENGINE.into(),
                    size_bytes: Some(dir_size(&nemotron).unwrap_or(0) + dir_size(&parakeet).unwrap_or(0)),
                    engine: Engine::LivePair { helper, nemotron, parakeet, chunk_ms },
                        });
            } else {
                unavailable.push(Unavailable {
                    id: "live-pair".into(),
                    reason: format!("the {chunk_ms} ms Nemotron or Parakeet folder is missing"),
                });
            }
        }
        (helper, _) => unavailable.push(Unavailable {
            id: "live-pair".into(),
            reason: if !live_pair::platform_supported() {
                "requires Apple Silicon and macOS 14".into()
            } else if helper.is_none() {
                "helper missing (npm run build:fluid)".into()
            } else {
                "models not downloaded".into()
            },
        }),
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((models, unavailable))
}

fn glob_match(pattern: &str, value: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == value,
        Some((prefix, rest)) => value.strip_prefix(prefix).is_some_and(|tail| {
            (0..=tail.len())
                .filter(|&index| tail.is_char_boundary(index))
                .any(|index| glob_match(rest, &tail[index..]))
        }),
    }
}

/// Applies `--models a,b*,…` ("all" or empty keeps everything).
pub fn select(models: Vec<BenchModel>, filter: Option<&str>) -> Result<Vec<BenchModel>> {
    let Some(filter) = filter.filter(|f| !f.is_empty() && *f != "all") else {
        return Ok(models);
    };
    let patterns: Vec<&str> = filter.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    for pattern in &patterns {
        if !models.iter().any(|model| glob_match(pattern, &model.id)) {
            bail!(
                "No runnable model matches '{pattern}'. Available: {}",
                models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(models
        .into_iter()
        .filter(|model| patterns.iter().any(|pattern| glob_match(pattern, &model.id)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn globs_match_prefixes_suffixes_and_exact_ids() {
        assert!(glob_match("whisper-*", "whisper-turbo-q5"));
        assert!(glob_match("*-q5", "whisper-turbo-q5"));
        assert!(glob_match("live-pair", "live-pair"));
        assert!(!glob_match("live-pair", "live-pair-stream"));
        assert!(glob_match("*turbo*", "whisper-turbo-q5"));
        assert!(!glob_match("qwen*", "whisper-small"));
    }
}
