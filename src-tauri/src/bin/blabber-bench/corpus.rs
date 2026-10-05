//! The test corpus: `_private/asr-corpus/manifest.json`, its audio, drafts and
//! the read-aloud scripts in `docs/asr-benchmark-scripts.json`.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use speech_to_text_lib::{asr_scoring, audio_preprocess};

use crate::paths::{display, write_atomic, Paths};

pub const SCRIPTS_JSON: &str = include_str!("../../../../docs/asr-benchmark-scripts.json");
pub const SHORT_MAX_MS: u64 = 60_000;

pub const CONVENTIONS: &str = "Write what was actually said, the way you would type it. \
Numbers may be digits or words (\"9:30\" and \"neun Uhr dreißig\" score the same), but do not change \
what was said: \"halb zehn\" is not \"9:30\". Keep slips and repeated words if they were spoken. \
Fillers (äh, ähm, um, uh, euh) are ignored by the scoring, so including them or not makes no difference. \
Use normal punctuation and capitalisation. No-speech clips have an empty reference.";

// MARK: Scripts

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub id: String,
    pub language: String,
    pub languages: Vec<String>,
    pub category: String,
    pub tags: Vec<String>,
    pub terms: Vec<String>,
    pub text: String,
    #[serde(default)]
    pub segments: Option<Vec<Segment>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptVariant {
    id: String,
    condition: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptFile {
    scripts: Vec<Script>,
    variants: Vec<ScriptVariant>,
}

pub struct Scripts {
    scripts: HashMap<String, Script>,
    conditions: HashMap<String, String>,
}

impl Scripts {
    pub fn load() -> Result<Self> {
        let file: ScriptFile =
            serde_json::from_str(SCRIPTS_JSON).context("docs/asr-benchmark-scripts.json is invalid")?;
        Ok(Self {
            conditions: file.variants.into_iter().map(|v| (v.id, v.condition)).collect(),
            scripts: file.scripts.into_iter().map(|s| (s.id.clone(), s)).collect(),
        })
    }

    /// Resolves "de-s05" or "de-s05@noisy" to its script and condition.
    pub fn resolve(&self, id: &str) -> Option<(&Script, Option<String>)> {
        let (base, condition) = match id.split_once('@') {
            Some((base, condition)) => (base, Some(condition.to_string())),
            None => (id, None),
        };
        let script = self.scripts.get(base)?;
        if let Some(condition) = &condition {
            if !self.conditions.contains_key(id) && !condition.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return None;
            }
        }
        Some((script, condition))
    }

    pub fn text(&self, id: &str) -> Option<&str> {
        self.resolve(id).map(|(script, _)| script.text.as_str())
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }
}

// MARK: Manifest

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub lang: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    /// "de" | "en" | "fr" | "mixed" | "none"
    pub language: String,
    pub languages: Vec<String>,
    /// "short" | "long"
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub speaker: Option<String>,
    pub duration_ms: u64,
    pub audio_sha256: String,
    /// Relative to the corpus directory.
    pub audio: String,
    #[serde(default)]
    pub original: Option<String>,
    pub reference: String,
    /// "draft" | "verified"
    pub reference_status: String,
    #[serde(default)]
    pub reference_drafted_by: Vec<String>,
    #[serde(default)]
    pub segments: Option<Vec<Segment>>,
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl Clip {
    pub fn verified(&self) -> bool {
        self.reference_status == "verified"
    }

    pub fn langs(&self) -> Vec<asr_scoring::Lang> {
        self.languages
            .iter()
            .filter_map(|code| asr_scoring::Lang::from_code(code))
            .collect()
    }

    /// Segments only stay valid while the reference is the script text.
    pub fn scoring_segments(&self) -> Option<Vec<(asr_scoring::Lang, String)>> {
        let segments = self.segments.as_ref()?;
        let joined = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        if squash(&joined) != squash(&self.reference) {
            return None;
        }
        segments
            .iter()
            .map(|s| asr_scoring::Lang::from_code(&s.lang).map(|lang| (lang, s.text.clone())))
            .collect()
    }

    pub fn audio_path(&self, corpus: &Path) -> PathBuf {
        corpus.join(&self.audio)
    }
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub clips: Vec<Clip>,
}

impl Manifest {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.manifest();
        if !path.is_file() {
            return Ok(Self { version: 1, clips: Vec::new() });
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text).with_context(|| format!("{} is invalid", path.display()))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_atomic(&paths.manifest(), text.as_bytes())
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|clip| clip.id == id)
    }
}

// MARK: Drafts

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub model: String,
    pub text: String,
    pub segments: Vec<DraftSegment>,
}

pub fn drafts_path(paths: &Paths, id: &str) -> PathBuf {
    paths.corpus.join("drafts").join(format!("{id}.json"))
}

pub fn load_drafts(paths: &Paths, id: &str) -> Vec<Draft> {
    std::fs::read_to_string(drafts_path(paths, id))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_drafts(paths: &Paths, id: &str, drafts: &[Draft]) -> Result<()> {
    write_atomic(&drafts_path(paths, id), serde_json::to_string_pretty(drafts)?.as_bytes())
}

// MARK: Import

pub struct ImportOptions {
    pub language: Option<String>,
    pub tags: Vec<String>,
    pub speaker: Option<String>,
    pub force: bool,
}

fn sanitize_id(stem: &str) -> String {
    let mut id: String = stem
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '@') { c } else { '-' })
        .collect();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    id.trim_matches('-').to_lowercase()
}

fn language_from_prefix(id: &str) -> Option<String> {
    let prefix = id.split(['-', '_']).next()?;
    match prefix {
        "de" | "en" | "fr" => Some(prefix.into()),
        "mx" | "mixed" => Some("mixed".into()),
        "noise" | "silence" => Some("none".into()),
        _ => None,
    }
}

fn expand_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(input)?
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| path.is_file() && audio_preprocess::is_supported_media_path(path))
                .collect();
            entries.sort();
            files.extend(entries);
        } else if input.is_file() {
            files.push(input.clone());
        } else {
            bail!("{} does not exist", input.display());
        }
    }
    Ok(files)
}

pub fn import(paths: &Paths, inputs: &[PathBuf], options: &ImportOptions) -> Result<()> {
    let scripts = Scripts::load()?;
    let mut manifest = Manifest::load(paths)?;
    let files = expand_inputs(inputs)?;
    if files.is_empty() {
        bail!("No audio files found in the given paths.");
    }
    let (mut added, mut skipped) = (0, 0);
    for file in files {
        let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let id = sanitize_id(&stem);
        if id.is_empty() {
            eprintln!("  skip {}: no usable name", file.display());
            skipped += 1;
            continue;
        }
        if manifest.clips.iter().any(|clip| clip.id == id) && !options.force {
            eprintln!("  skip {id}: already imported (use --force to replace)");
            skipped += 1;
            continue;
        }
        let audio = audio_preprocess::decode_audio_file(&file)
            .with_context(|| format!("could not decode {}", file.display()))?;
        let duration_ms = audio.samples.len() as u64 * 1000 / 16_000;
        let relative_audio = format!("audio/{id}.wav");
        let wav = paths.corpus.join(&relative_audio);
        audio_preprocess::write_wav(&wav, &audio)?;
        let extension = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "bin".into());
        let original = format!("originals/{id}.{extension}");
        std::fs::create_dir_all(paths.corpus.join("originals"))?;
        std::fs::copy(&file, paths.corpus.join(&original))?;
        let sha = audio_preprocess::sha256_file(&wav)?;

        let clip = match scripts.resolve(&id) {
            Some((script, condition)) => {
                let silent = script.language == "none";
                let mut tags = script.tags.clone();
                tags.extend(options.tags.iter().cloned());
                tags.sort();
                tags.dedup();
                Clip {
                    id: id.clone(),
                    language: script.language.clone(),
                    languages: script.languages.clone(),
                    category: script.category.clone(),
                    tags,
                    condition,
                    script: Some(script.id.clone()),
                    speaker: options.speaker.clone(),
                    duration_ms,
                    audio_sha256: sha,
                    audio: relative_audio,
                    original: Some(original),
                    reference: script.text.clone(),
                    reference_status: if silent { "verified" } else { "draft" }.into(),
                    reference_drafted_by: vec!["script".into()],
                    segments: script.segments.clone(),
                    terms: script.terms.clone(),
                    notes: String::new(),
                }
            }
            None => {
                let language = options
                    .language
                    .clone()
                    .or_else(|| language_from_prefix(&id))
                    .with_context(|| {
                        format!("{id}: not a script id; pass --lang de|en|fr|mixed|none or prefix the file name with de-, en-, fr-, mx-")
                    })?;
                let languages = match language.as_str() {
                    "mixed" => vec!["de".to_string(), "en".to_string()],
                    "none" => vec![],
                    code => vec![code.to_string()],
                };
                Clip {
                    id: id.clone(),
                    languages,
                    category: if duration_ms <= SHORT_MAX_MS { "short" } else { "long" }.into(),
                    tags: options.tags.clone(),
                    condition: None,
                    script: None,
                    speaker: options.speaker.clone(),
                    duration_ms,
                    audio_sha256: sha,
                    audio: relative_audio,
                    original: Some(original),
                    reference: String::new(),
                    reference_status: if language == "none" { "verified" } else { "draft" }.into(),
                    reference_drafted_by: vec![],
                    segments: None,
                    terms: vec![],
                    notes: String::new(),
                    language,
                }
            }
        };
        println!(
            "  {:<22} {:>6.1} s  {:<6} {:<5} {}",
            clip.id,
            duration_ms as f64 / 1000.0,
            clip.language,
            clip.category,
            if clip.script.is_some() { "script" } else { "free speech" }
        );
        manifest.clips.retain(|existing| existing.id != clip.id);
        manifest.clips.push(clip);
        added += 1;
    }
    manifest.clips.sort_by(|a, b| a.id.cmp(&b.id));
    manifest.save(paths)?;
    println!(
        "\nImported {added} clip(s), skipped {skipped}. Manifest: {}",
        display(&paths.manifest(), &paths.repo)
    );
    println!("Next: `npm run bench -- corpus draft`, then `npm run bench -- corpus review`.");
    Ok(())
}

// MARK: Synthetic corpus

/// The macOS `say` fixtures from `workers/r2t2/make_probe_audio.py --individual`,
/// as verified clips (for smoke runs without recordings).
pub fn synthetic(paths: &Paths) -> Result<Vec<Clip>> {
    if !paths.fixtures.is_dir() {
        bail!(
            "Synthetic fixtures are missing. Create them with `python3 workers/r2t2/make_probe_audio.py --individual`."
        );
    }
    let mut clips = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&paths.fixtures)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "wav"))
        .collect();
    entries.sort();
    for wav in entries {
        let id = wav.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let reference_path = wav.with_extension("txt");
        let Some(language) = language_from_prefix(&id).filter(|l| l != "none") else { continue };
        if id.contains("synthetic") || !reference_path.is_file() {
            continue;
        }
        let reference = std::fs::read_to_string(&reference_path)?.trim().to_string();
        let audio = audio_preprocess::decode_wav_file(&wav)?;
        let duration_ms = audio.samples.len() as u64 * 1000 / 16_000;
        let languages = if language == "mixed" { vec!["de".into(), "en".into()] } else { vec![language.clone()] };
        clips.push(Clip {
            id: format!("syn-{id}"),
            language,
            languages,
            category: if duration_ms <= SHORT_MAX_MS { "short" } else { "long" }.into(),
            tags: vec!["synthetic".into()],
            condition: None,
            script: None,
            speaker: Some("macOS say".into()),
            duration_ms,
            audio_sha256: audio_preprocess::sha256_file(&wav)?,
            audio: wav.display().to_string(),
            original: None,
            reference,
            reference_status: "verified".into(),
            reference_drafted_by: vec![],
            segments: None,
            terms: vec![],
            notes: String::new(),
        });
    }
    if clips.is_empty() {
        bail!("No usable fixtures in {}", paths.fixtures.display());
    }
    Ok(clips)
}

// MARK: Check

pub fn check(paths: &Paths) -> Result<bool> {
    let manifest = Manifest::load(paths)?;
    let scripts = Scripts::load()?;
    if manifest.clips.is_empty() {
        println!("The corpus is empty. Import recordings with `npm run bench -- corpus import <folder>`.");
        return Ok(false);
    }
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    let mut table: BTreeMap<(String, String), (usize, usize, u64)> = BTreeMap::new();
    let mut seen = HashMap::new();
    for clip in &manifest.clips {
        *seen.entry(clip.id.clone()).or_insert(0) += 1;
        let entry = table.entry((clip.language.clone(), clip.category.clone())).or_default();
        entry.0 += 1;
        entry.1 += clip.verified() as usize;
        entry.2 += clip.duration_ms;
        let wav = clip.audio_path(&paths.corpus);
        if !wav.is_file() {
            problems.push(format!("{}: audio file missing ({})", clip.id, clip.audio));
            continue;
        }
        match audio_preprocess::sha256_file(&wav) {
            Ok(sha) if sha != clip.audio_sha256 => {
                problems.push(format!("{}: audio changed since import (hash mismatch)", clip.id))
            }
            Err(error) => problems.push(format!("{}: {error}", clip.id)),
            _ => {}
        }
        if !["short", "long"].contains(&clip.category.as_str()) {
            problems.push(format!("{}: unknown category '{}'", clip.id, clip.category));
        }
        if clip.category == "short" && clip.duration_ms > SHORT_MAX_MS {
            notes.push(format!("{}: marked short but {:.0} s long", clip.id, clip.duration_ms as f64 / 1000.0));
        }
        if clip.language != "none" && clip.verified() && clip.reference.trim().is_empty() {
            problems.push(format!("{}: verified with an empty reference", clip.id));
        }
        if clip.language != "none" && clip.duration_ms < 800 {
            notes.push(format!("{}: only {} ms of audio", clip.id, clip.duration_ms));
        }
        if let Some(script) = clip.script.as_deref().and_then(|id| scripts.text(id)) {
            if clip.verified() && squash(script) != squash(&clip.reference) {
                notes.push(format!("{}: reference edited from the script (expected after review)", clip.id));
            }
        }
    }
    for (id, count) in seen {
        if count > 1 {
            problems.push(format!("{id}: appears {count} times"));
        }
    }
    println!("{:<8} {:<6} {:>6} {:>9} {:>9}", "language", "type", "clips", "verified", "audio");
    for ((language, category), (count, verified, duration)) in &table {
        println!(
            "{:<8} {:<6} {:>6} {:>9} {:>7.1} min",
            language,
            category,
            count,
            verified,
            *duration as f64 / 60_000.0
        );
    }
    let verified = manifest.clips.iter().filter(|c| c.verified()).count();
    println!("\n{verified} of {} clips verified.", manifest.clips.len());
    for note in &notes {
        println!("  note: {note}");
    }
    for problem in &problems {
        println!("  PROBLEM: {problem}");
    }
    if problems.is_empty() {
        println!("No problems found.");
    }
    Ok(problems.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_resolve_with_conditions() {
        let scripts = Scripts::load().unwrap();
        assert!(scripts.len() > 40);
        let (script, condition) = scripts.resolve("de-s05@noisy").unwrap();
        assert_eq!(script.id, "de-s05");
        assert_eq!(condition.as_deref(), Some("noisy"));
        assert!(scripts.resolve("de-s99").is_none());
        assert_eq!(scripts.resolve("mx-l01").unwrap().0.language, "mixed");
    }

    #[test]
    fn ids_and_languages_come_from_file_names() {
        assert_eq!(sanitize_id("DE s05 (take 2)"), "de-s05-take-2");
        assert_eq!(sanitize_id("de-s05@noisy"), "de-s05@noisy");
        assert_eq!(language_from_prefix("mx-free-1").as_deref(), Some("mixed"));
        assert_eq!(language_from_prefix("meeting"), None);
    }
}
