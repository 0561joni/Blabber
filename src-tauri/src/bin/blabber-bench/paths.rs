//! Where the benchmark finds models, helpers, the corpus and its runs.
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

pub const APP_IDENTIFIER: &str = "com.jonibuch.speechtotext";

#[derive(Debug, Clone)]
pub struct Paths {
    pub repo: PathBuf,
    pub manifest_dir: PathBuf,
    pub models: PathBuf,
    pub db: PathBuf,
    pub corpus: PathBuf,
    pub runs: PathBuf,
    pub summaries: PathBuf,
    pub fixtures: PathBuf,
}

impl Paths {
    pub fn new(models: Option<PathBuf>, corpus: Option<PathBuf>, runs: Option<PathBuf>) -> Result<Self> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest_dir
            .parent()
            .context("source tree has no parent")?
            .to_path_buf();
        let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?;
        let app_data = home.join("Library/Application Support").join(APP_IDENTIFIER);
        Ok(Self {
            models: models.unwrap_or_else(|| app_data.join("models")),
            db: app_data.join("speech_to_text.sqlite"),
            corpus: corpus.unwrap_or_else(|| repo.join("_private/asr-corpus")),
            runs: runs.unwrap_or_else(|| repo.join("_private/asr-bench/runs")),
            summaries: repo.join("benchmark-results"),
            fixtures: manifest_dir.join("target/r2t2-fixtures"),
            manifest_dir,
            repo,
        })
    }

    /// First existing candidate among a helper's packaged and development paths.
    pub fn helper(&self, relative_bundle: &str, packaged: &str) -> Option<PathBuf> {
        [
            self.manifest_dir.join("bundle").join(relative_bundle),
            PathBuf::from("/Applications/Blabber.app/Contents/Resources").join(packaged),
        ]
        .into_iter()
        .find(|path| path.is_file())
    }

    pub fn manifest(&self) -> PathBuf {
        self.corpus.join("manifest.json")
    }

    pub fn temp(&self) -> PathBuf {
        self.runs.parent().unwrap_or(&self.runs).join("tmp")
    }
}

/// `target` relative to the directory `from`, for links inside generated pages.
pub fn relative(from: &Path, target: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let target_parts: Vec<Component> = target.components().collect();
    let common = from
        .iter()
        .zip(&target_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for part in &target_parts[common..] {
        path.push(part.as_os_str());
    }
    path
}

pub fn display(path: &Path, repo: &Path) -> String {
    path.strip_prefix(repo)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// Writes through a temporary file so readers never see half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_climb_to_the_common_ancestor() {
        assert_eq!(
            relative(
                Path::new("/r/_private/asr-bench/runs/x"),
                Path::new("/r/_private/asr-corpus/audio/a.wav")
            ),
            PathBuf::from("../../../asr-corpus/audio/a.wav")
        );
    }
}
