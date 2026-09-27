use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};
use unicode_normalization::UnicodeNormalization;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const APP_IDENTIFIER: &str = "com.jonibuch.speechtotext";
const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;
const MAX_INPUT_FILES: usize = 3;
const MIN_MODEL_BYTES: u64 = 1_000_000;
const DEFAULT_REFERENCE_MODEL: &str = "gpt-4o-transcribe";
const DEFAULT_JUDGE_MODEL: &str = "gpt-5-mini";
const NORMALIZATION_VERSION: &str = "nfkc-lower-punct-v1";
const CHUNKING_VERSION: &str = "silence-aware-wav24mb-v1";
const SEMANTIC_RUBRIC_VERSION: &str = "semantic-rubric-v1";
const REFERENCE_CACHE_DIR: &str = "benchmark-results/reference-cache";
const SEMANTIC_CACHE_DIR: &str = "benchmark-results/semantic-cache";
const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const SAFE_CHUNK_LIMIT_BYTES: usize = 24 * 1024 * 1024;
const WAV_HEADER_BYTES: usize = 44;
const PCM16_BYTES_PER_SAMPLE: usize = 2;
const SILENCE_THRESHOLD: f32 = 0.015;
const SILENCE_MIN_MS: usize = 250;
const SILENCE_SEARCH_WINDOW_SECONDS: usize = 8;
const PROMPT_TAIL_CHARS: usize = 224;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_usage();
        bail!("missing command");
    }

    match args[0].to_string_lossy().as_ref() {
        "compare" => compare_command(&args[1..]),
        "evaluate" => evaluate_command(&args[1..]),
        "run-single" => run_single_command(&args[1..]),
        "--help" | "-h" | "help" => {
            print_usage();
            Ok(())
        }
        other => {
            print_usage();
            bail!("unknown command: {other}");
        }
    }
}

fn compare_command(raw_args: &[OsString]) -> Result<()> {
    let parsed = parse_common_args(raw_args, false, false)?;
    validate_input_count(&parsed.inputs)?;

    let json_out = parsed
        .json_out
        .clone()
        .unwrap_or_else(default_json_output_path);
    ensure_parent_directory(&json_out)?;

    let merged = run_compare_workflow(&parsed)?;
    write_json(&json_out, &merged)?;
    print_terminal_report(&merged);
    println!("\nJSON report: {}", json_out.display());

    if merged.backend_run_errors.is_empty() {
        Ok(())
    } else {
        bail!("{}", merged.backend_run_errors.join("\n"))
    }
}

fn evaluate_command(raw_args: &[OsString]) -> Result<()> {
    let parsed = parse_common_args(raw_args, false, false)?;
    validate_input_count(&parsed.inputs)?;

    let api_key = env::var("OPENAI_API_KEY")
        .context("evaluate requires OPENAI_API_KEY to be set in the environment")?;

    let json_out = parsed
        .json_out
        .clone()
        .unwrap_or_else(default_json_output_path);
    ensure_parent_directory(&json_out)?;

    let input_paths = canonicalize_inputs(&parsed.inputs)?;
    let prepared_inputs = input_paths
        .iter()
        .map(|path| prepare_input_audio(path))
        .collect::<Result<Vec<_>>>()?;

    let base_report = run_compare_workflow(&parsed)?;
    let client = OpenAiClient::new(api_key)?;
    let evaluation_config = EvaluationConfig::from_args(&parsed);
    let evaluated_report =
        augment_report_with_evaluation(base_report, &prepared_inputs, &client, &evaluation_config)?;

    write_json(&json_out, &evaluated_report)?;
    print_terminal_report(&evaluated_report);
    println!("\nJSON report: {}", json_out.display());

    let mut errors = Vec::new();
    errors.extend(evaluated_report.backend_run_errors.clone());
    errors.extend(evaluated_report.api_run_errors.clone());
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("{}", errors.join("\n"))
    }
}

fn run_compare_workflow(parsed: &ParsedArgs) -> Result<MergedReport> {
    let models_dir = resolve_models_dir(parsed.models_dir.clone())?;
    let input_paths = canonicalize_inputs(&parsed.inputs)?;

    let cpu_json = temp_report_path("cpu");
    let metal_json = temp_report_path("metal");

    let cpu_report = execute_backend_run(
        Backend::Cpu,
        &input_paths,
        &models_dir,
        &cpu_json,
        parsed.timestamps,
    )?;

    let mut backend_errors = Vec::new();
    let metal_report = match execute_backend_run(
        Backend::Metal,
        &input_paths,
        &models_dir,
        &metal_json,
        parsed.timestamps,
    ) {
        Ok(report) => Some(report),
        Err(error) => {
            backend_errors.push(format!("Metal run failed: {error:#}"));
            None
        }
    };

    Ok(merge_reports(
        cpu_report,
        metal_report,
        backend_errors,
        &models_dir,
    ))
}

fn run_single_command(raw_args: &[OsString]) -> Result<()> {
    let parsed = parse_common_args(raw_args, true, true)?;
    validate_input_count(&parsed.inputs)?;

    let backend = parsed.backend.context("missing --backend")?;
    ensure_backend_matches_build(backend)?;

    let models_dir = resolve_models_dir(parsed.models_dir)?;
    let input_paths = canonicalize_inputs(&parsed.inputs)?;
    let json_out = parsed.json_out.context("missing --json-out")?;
    ensure_parent_directory(&json_out)?;

    let prepared_inputs = input_paths
        .iter()
        .map(|path| prepare_input_audio(path))
        .collect::<Result<Vec<_>>>()?;
    let models = discover_models(&models_dir)?;
    if models.is_empty() {
        bail!(
            "no benchmarkable whisper.cpp models were found in {}",
            models_dir.display()
        );
    }

    let metadata = collect_machine_metadata(&models_dir)?;
    let mut results = Vec::new();
    for input in &prepared_inputs {
        for model in &models {
            results.push(benchmark_model(model, input, backend, parsed.timestamps));
        }
    }

    let report = BackendRunReport {
        run_metadata: metadata,
        inputs: prepared_inputs
            .iter()
            .map(|input| InputSummary {
                path: input.path.display().to_string(),
                file_name: input.file_name.clone(),
                audio_duration_ms: input.audio_duration_ms,
            })
            .collect(),
        models: models
            .iter()
            .map(|model| ModelSummary {
                model_name: model.model_name.clone(),
                model_path: model.model_path.display().to_string(),
                size_bytes: model.size_bytes,
            })
            .collect(),
        results,
    };

    write_json(&json_out, &report)?;
    Ok(())
}

fn ensure_backend_matches_build(backend: Backend) -> Result<()> {
    match backend {
        Backend::Cpu if cfg!(feature = "metal") => {
            bail!("cpu benchmark must be run without the metal Cargo feature")
        }
        Backend::Metal if !cfg!(feature = "metal") => {
            bail!("metal benchmark must be run with the metal Cargo feature enabled")
        }
        _ => Ok(()),
    }
}

fn execute_backend_run(
    backend: Backend,
    inputs: &[PathBuf],
    models_dir: &Path,
    json_out: &Path,
    timestamps: bool,
) -> Result<BackendRunReport> {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest_path);

    if backend == Backend::Metal {
        command.arg("--features").arg("metal");
    }

    command.arg("--").arg("run-single");
    command.arg("--backend").arg(backend.as_str());
    command.arg("--models-dir").arg(models_dir);
    command.arg("--json-out").arg(json_out);
    if timestamps {
        command.arg("--timestamps");
    }
    for input in inputs {
        command.arg("--input").arg(input);
    }

    let output = command
        .output()
        .with_context(|| format!("failed to spawn cargo for {} benchmark", backend.as_str()))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} benchmark subprocess failed.\nstdout:\n{}\nstderr:\n{}",
            backend.as_str(),
            stdout.trim(),
            stderr.trim()
        );
    }

    let file = File::open(json_out)
        .with_context(|| format!("failed to read {} result file", backend.as_str()))?;
    serde_json::from_reader(file)
        .with_context(|| format!("failed to parse {} benchmark json", backend.as_str()))
}

fn parse_common_args(
    raw_args: &[OsString],
    require_json_out: bool,
    require_backend: bool,
) -> Result<ParsedArgs> {
    let mut inputs = Vec::new();
    let mut models_dir = None;
    let mut json_out = None;
    let mut timestamps = false;
    let mut backend = None;
    let mut openai_model = DEFAULT_REFERENCE_MODEL.to_string();
    let mut judge_model = DEFAULT_JUDGE_MODEL.to_string();
    let mut refresh_reference = false;
    let mut skip_semantic_judge = false;

    let mut index = 0;
    while index < raw_args.len() {
        let current = raw_args[index].to_string_lossy();
        match current.as_ref() {
            "--input" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--input requires a path"))?;
                inputs.push(PathBuf::from(value));
            }
            "--models-dir" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--models-dir requires a path"))?;
                models_dir = Some(PathBuf::from(value));
            }
            "--json-out" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--json-out requires a path"))?;
                json_out = Some(PathBuf::from(value));
            }
            "--backend" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--backend requires cpu or metal"))?;
                backend = Some(Backend::parse(value)?);
            }
            "--openai-model" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--openai-model requires a model id"))?;
                openai_model = value.to_string_lossy().to_string();
            }
            "--judge-model" => {
                index += 1;
                let value = raw_args
                    .get(index)
                    .ok_or_else(|| anyhow!("--judge-model requires a model id"))?;
                judge_model = value.to_string_lossy().to_string();
            }
            "--timestamps" => {
                timestamps = true;
            }
            "--refresh-reference" => {
                refresh_reference = true;
            }
            "--skip-semantic-judge" => {
                skip_semantic_judge = true;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
        index += 1;
    }

    if require_json_out && json_out.is_none() {
        bail!("run-single requires --json-out");
    }
    if require_backend && backend.is_none() {
        bail!("run-single requires --backend");
    }

    Ok(ParsedArgs {
        inputs,
        models_dir,
        json_out,
        timestamps,
        backend,
        openai_model,
        judge_model,
        refresh_reference,
        skip_semantic_judge,
    })
}

fn validate_input_count(inputs: &[PathBuf]) -> Result<()> {
    if inputs.is_empty() {
        bail!("provide at least one --input file");
    }
    if inputs.len() > MAX_INPUT_FILES {
        bail!("provide at most {MAX_INPUT_FILES} input files");
    }
    Ok(())
}

fn canonicalize_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    inputs
        .iter()
        .map(|path| {
            fs::canonicalize(path)
                .with_context(|| format!("failed to resolve input file {}", path.display()))
        })
        .collect()
}

fn resolve_models_dir(override_path: Option<PathBuf>) -> Result<PathBuf> {
    let path = override_path.unwrap_or_else(default_models_dir);
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to ensure models directory {}", path.display()))?;
    Ok(path)
}

fn default_models_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library")
        .join("Application Support")
        .join(APP_IDENTIFIER)
        .join("models")
}

fn default_json_output_path() -> PathBuf {
    PathBuf::from("benchmark-results").join(format!("{}-model-bench.json", unix_timestamp_ms()))
}

fn reference_cache_dir() -> PathBuf {
    PathBuf::from(REFERENCE_CACHE_DIR)
}

fn semantic_cache_dir() -> PathBuf {
    PathBuf::from(SEMANTIC_CACHE_DIR)
}

fn temp_report_path(label: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "model-bench-{}-{}-{}.json",
        label,
        std::process::id(),
        unix_timestamp_ms()
    ))
}

fn ensure_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn discover_models(models_dir: &Path) -> Result<Vec<ModelSpec>> {
    let mut models = fs::read_dir(models_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| model_from_path(entry.path()).transpose())
        .collect::<Result<Vec<_>>>()?;

    models.sort_by(|left, right| left.model_name.cmp(&right.model_name));
    Ok(models)
}

fn model_from_path(path: PathBuf) -> Result<Option<ModelSpec>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if extension != "bin" {
        return Ok(None);
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("model file name is not valid UTF-8: {}", path.display()))?;
    if file_name.starts_with("stub-") {
        return Ok(None);
    }

    let metadata = fs::metadata(&path)?;
    if metadata.len() < MIN_MODEL_BYTES {
        return Ok(None);
    }

    Ok(Some(ModelSpec {
        model_name: file_name.to_string(),
        model_path: path,
        size_bytes: metadata.len(),
    }))
}

fn prepare_input_audio(path: &Path) -> Result<PreparedInput> {
    validate_audio_path(path)?;
    let decoded = decode_audio_file(path)?;
    let normalized = normalize_audio(&decoded.samples, decoded.sample_rate_hz, decoded.channels);
    let audio_duration_ms = duration_ms(normalized.samples.len());
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("input file name is not valid UTF-8: {}", path.display()))?
        .to_string();

    Ok(PreparedInput {
        path: path.to_path_buf(),
        file_name,
        audio_duration_ms,
        audio: normalized.samples,
    })
}

fn validate_audio_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("input file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("input path is not a file: {}", path.display());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if !matches!(extension.as_str(), "wav" | "mp3" | "m4a") {
        bail!("unsupported input format: {}", path.display());
    }
    Ok(())
}

fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let media_source_stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = get_probe()
        .format(
            &hint,
            media_source_stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("failed to open audio container for {}", path.display()))?;

    let mut format = probed.format;
    let (track_id, codec_params) = {
        let track = format
            .default_track()
            .context("no default audio track found in selected file")?;
        (track.id, track.codec_params.clone())
    };

    let mut sample_rate_hz = codec_params.sample_rate;
    let mut channels = codec_params
        .channels
        .map(|value| value.count() as u16)
        .filter(|value| *value > 0);

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .context("failed to initialize audio decoder")?;
    let mut samples = Vec::<f32>::new();
    let mut sample_buffer = None::<SampleBuffer<f32>>;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                bail!("audio stream changed mid-file and cannot be decoded safely")
            }
            Err(error) => {
                return Err(anyhow!(error))
                    .with_context(|| format!("failed while reading {}", path.display()));
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => {
                return Err(anyhow!(error))
                    .with_context(|| format!("failed while decoding {}", path.display()));
            }
        };

        let spec = *decoded.spec();
        if sample_rate_hz.is_none() {
            sample_rate_hz = Some(spec.rate);
        }
        if channels.is_none() {
            let decoded_channels = spec.channels.count() as u16;
            if decoded_channels > 0 {
                channels = Some(decoded_channels);
            }
        }

        let duration = decoded.capacity() as u64;
        let buffer = sample_buffer.get_or_insert_with(|| SampleBuffer::<f32>::new(duration, spec));
        buffer.copy_interleaved_ref(decoded);
        samples.extend_from_slice(buffer.samples());
    }

    Ok(DecodedAudio {
        sample_rate_hz: sample_rate_hz.ok_or_else(|| anyhow!("missing sample rate"))?,
        channels: channels.ok_or_else(|| anyhow!("missing channel information"))?,
        samples,
    })
}

fn normalize_audio(input: &[f32], input_sample_rate_hz: u32, input_channels: u16) -> PreparedAudio {
    let mono_samples = mix_to_mono(input, input_channels);
    let samples = if input_sample_rate_hz == TARGET_SAMPLE_RATE_HZ {
        mono_samples
    } else {
        resample_linear(&mono_samples, input_sample_rate_hz, TARGET_SAMPLE_RATE_HZ)
    };

    PreparedAudio { samples }
}

fn mix_to_mono(input: &[f32], input_channels: u16) -> Vec<f32> {
    if input_channels <= 1 {
        return input.to_vec();
    }
    let channels = input_channels as usize;
    input
        .chunks(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if input.is_empty() || from_hz == 0 || to_hz == 0 {
        return Vec::new();
    }
    if from_hz == to_hz {
        return input.to_vec();
    }

    let ratio = to_hz as f64 / from_hz as f64;
    let output_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut output = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let source_position = index as f64 / ratio;
        let left_index = source_position.floor() as usize;
        let right_index = (left_index + 1).min(input.len().saturating_sub(1));
        let weight = (source_position - left_index as f64) as f32;
        let left = input[left_index];
        let right = input[right_index];
        output.push(left + ((right - left) * weight));
    }
    output
}

fn benchmark_model(
    model: &ModelSpec,
    input: &PreparedInput,
    backend: Backend,
    timestamps: bool,
) -> BenchmarkRecord {
    let timer = Instant::now();
    let attempt = benchmark_model_inner(model, input, timestamps);
    let wall_time_ms = timer.elapsed().as_millis() as u64;

    match attempt {
        Ok(inference) => {
            let realtime_factor = wall_time_ms as f64 / input.audio_duration_ms as f64;
            let speed_multiplier = input.audio_duration_ms as f64 / wall_time_ms.max(1) as f64;
            BenchmarkRecord {
                backend,
                model_name: model.model_name.clone(),
                model_path: model.model_path.display().to_string(),
                input_path: input.path.display().to_string(),
                input_name: input.file_name.clone(),
                audio_duration_ms: input.audio_duration_ms,
                wall_time_ms: Some(wall_time_ms),
                realtime_factor: Some(realtime_factor),
                speed_multiplier: Some(speed_multiplier),
                transcript_length: Some(inference.transcript_text.chars().count()),
                transcript_text: Some(inference.transcript_text),
                timestamps_monotonic: Some(inference.timestamps_monotonic),
                speaker_sequence: inference.speaker_sequence,
                peak_memory_bytes: peak_memory_bytes(),
                cold_load_time_ms: Some(inference.cold_load_time_ms),
                warm_load_time_ms: inference.warm_load_time_ms,
                success: true,
                error: None,
                quality: None,
            }
        }
        Err(error) => BenchmarkRecord {
            backend,
            model_name: model.model_name.clone(),
            model_path: model.model_path.display().to_string(),
            input_path: input.path.display().to_string(),
            input_name: input.file_name.clone(),
            audio_duration_ms: input.audio_duration_ms,
            wall_time_ms: Some(wall_time_ms),
            realtime_factor: None,
            speed_multiplier: None,
            transcript_length: None,
            transcript_text: None,
            timestamps_monotonic: None,
            speaker_sequence: Vec::new(),
            peak_memory_bytes: peak_memory_bytes(),
            cold_load_time_ms: None,
            warm_load_time_ms: None,
            success: false,
            error: Some(format!("{error:#}")),
            quality: None,
        },
    }
}

fn benchmark_model_inner(
    model: &ModelSpec,
    input: &PreparedInput,
    timestamps: bool,
) -> Result<InferenceMetrics> {
    let cold_load_started = Instant::now();
    let context = WhisperContext::new_with_params(
        &model.model_path.display().to_string(),
        WhisperContextParameters::default(),
    )
    .with_context(|| format!("failed to load model {}", model.model_name))?;
    let cold_load_time_ms = cold_load_started.elapsed().as_millis() as u64;

    let warm_load_started = Instant::now();
    let mut state = context
        .create_state()
        .with_context(|| format!("failed to create state for {}", model.model_name))?;
    let warm_load_time_ms = warm_load_started.elapsed().as_millis() as u64;
    let mut params = build_params(timestamps);
    configure_language_params(&mut params, None);
    state
        .full(params, &input.audio)
        .with_context(|| format!("transcription failed for {}", model.model_name))?;

    if let Some((transcript_text, timestamps_monotonic)) = collect_transcript_metrics(&state)? {
        return Ok(InferenceMetrics {
            transcript_text,
            timestamps_monotonic,
            speaker_sequence: Vec::new(),
            cold_load_time_ms,
            warm_load_time_ms: Some(warm_load_time_ms),
        });
    }

    let detected_language = language_id_to_code(state.full_lang_id_from_state());
    if detected_language != "auto" {
        let mut retry_state = context
            .create_state()
            .with_context(|| format!("failed to create retry state for {}", model.model_name))?;
        let mut retry_params = build_params(timestamps);
        configure_language_params(&mut retry_params, Some(detected_language.as_str()));
        retry_state
            .full(retry_params, &input.audio)
            .with_context(|| format!("fallback transcription failed for {}", model.model_name))?;

        if let Some((transcript_text, timestamps_monotonic)) =
            collect_transcript_metrics(&retry_state)?
        {
            return Ok(InferenceMetrics {
                transcript_text,
                timestamps_monotonic,
                speaker_sequence: Vec::new(),
                cold_load_time_ms,
                warm_load_time_ms: Some(warm_load_time_ms),
            });
        }
    }

    bail!("TRANSCRIPTION_EMPTY: whisper produced no segments")
}

fn build_params(timestamps: bool) -> FullParams<'static, 'static> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);
    params.set_translate(false);
    params.set_token_timestamps(false);
    params.set_split_on_word(false);
    params.set_n_threads(available_threads());
    params.set_offset_ms(0);
    params.set_duration_ms(0);
    params.set_no_timestamps(!timestamps);
    params
}

fn configure_language_params<'a>(params: &mut FullParams<'a, 'a>, fixed_language: Option<&'a str>) {
    match fixed_language {
        Some(language) => {
            params.set_language(Some(language));
            params.set_detect_language(false);
        }
        None => {
            params.set_language(None);
            params.set_detect_language(true);
        }
    }
}

fn collect_transcript_metrics(state: &whisper_rs::WhisperState) -> Result<Option<(String, bool)>> {
    let segment_count = state.full_n_segments();
    let mut transcript_parts = Vec::new();
    let mut timestamps_monotonic = true;
    let mut previous_end = 0_i64;
    for index in 0..segment_count {
        let Some(segment) = state.get_segment(index) else {
            continue;
        };
        let text = segment.to_str_lossy()?.trim().to_string();
        let start = segment.start_timestamp();
        let end = segment.end_timestamp();
        timestamps_monotonic &= start >= previous_end && end >= start;
        previous_end = end.max(previous_end);
        if !text.is_empty() {
            transcript_parts.push(text);
        }
    }

    if transcript_parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some((transcript_parts.join(" "), timestamps_monotonic)))
    }
}

#[cfg(unix)]
fn peak_memory_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return None;
    }
    let rss = unsafe { usage.assume_init() }.ru_maxrss as u64;
    Some(if cfg!(target_os = "macos") {
        rss
    } else {
        rss * 1024
    })
}

#[cfg(not(unix))]
fn peak_memory_bytes() -> Option<u64> {
    None
}

fn augment_report_with_evaluation(
    mut report: MergedReport,
    prepared_inputs: &[PreparedInput],
    client: &OpenAiClient,
    config: &EvaluationConfig,
) -> Result<MergedReport> {
    let mut reference_transcripts = BTreeMap::new();
    let mut input_lookup = BTreeMap::new();
    let mut cache_stats = EvaluationCacheStats::default();
    let mut api_run_errors = Vec::new();

    for input in prepared_inputs {
        input_lookup.insert(input.path.display().to_string(), input.clone());
        match get_or_create_reference_transcript(input, client, config, &mut cache_stats.reference)
        {
            Ok(reference) => {
                reference_transcripts.insert(input.path.display().to_string(), reference);
            }
            Err(error) => {
                api_run_errors.push(format!(
                    "Reference transcription failed for {}: {error:#}",
                    input.file_name
                ));
            }
        }
    }

    for result in &mut report.results {
        let reference = reference_transcripts.get(&result.input_path);
        let input = input_lookup.get(&result.input_path);
        result.quality = Some(score_benchmark_result(
            result,
            reference,
            input,
            client,
            config,
            &mut cache_stats.semantic,
            &mut api_run_errors,
        ));
    }

    report.reference_transcripts = reference_transcripts;
    report.evaluation_metadata = Some(EvaluationMetadata {
        reference_model: config.reference_model.clone(),
        judge_model: config.judge_model.clone(),
        normalization_version: NORMALIZATION_VERSION.to_string(),
        chunking_version: CHUNKING_VERSION.to_string(),
        semantic_rubric_version: SEMANTIC_RUBRIC_VERSION.to_string(),
        semantic_judge_enabled: !config.skip_semantic_judge,
        reference_cache_hits: cache_stats.reference.hits,
        reference_cache_misses: cache_stats.reference.misses,
        semantic_cache_hits: cache_stats.semantic.hits,
        semantic_cache_misses: cache_stats.semantic.misses,
    });
    report.api_run_errors = api_run_errors;
    Ok(report)
}

fn score_benchmark_result(
    result: &BenchmarkRecord,
    reference: Option<&ReferenceTranscriptRecord>,
    input: Option<&PreparedInput>,
    client: &OpenAiClient,
    config: &EvaluationConfig,
    semantic_cache_stats: &mut CacheStats,
    api_run_errors: &mut Vec<String>,
) -> QualityReport {
    let Some(reference) = reference else {
        return QualityReport {
            lexical_accuracy_pct: 0,
            semantic_accuracy_pct: None,
            overall_accuracy_pct: 0,
            wer: 1.0,
            cer: 1.0,
            reference_token_count: 0,
            candidate_token_count: 0,
            normalization_version: NORMALIZATION_VERSION.to_string(),
            judge_model: None,
            judge_rationale: Some("Reference transcript unavailable.".to_string()),
            critical_mismatches: vec!["Reference transcript unavailable.".to_string()],
        };
    };

    let reference_normalized = NormalizedTranscript::from_cached(
        reference.normalized_text.clone(),
        reference.reference_token_count,
    );
    let candidate_text = result.transcript_text.as_deref().unwrap_or_default();
    let candidate_normalized = normalize_for_scoring(candidate_text);
    let (wer, cer, lexical_accuracy_pct) =
        compute_lexical_metrics(&reference_normalized, &candidate_normalized);

    if !result.success || candidate_text.trim().is_empty() {
        let reason = result
            .error
            .clone()
            .unwrap_or_else(|| "Candidate transcript unavailable.".to_string());
        return QualityReport {
            lexical_accuracy_pct: 0,
            semantic_accuracy_pct: Some(0),
            overall_accuracy_pct: 0,
            wer,
            cer,
            reference_token_count: reference_normalized.tokens.len(),
            candidate_token_count: 0,
            normalization_version: NORMALIZATION_VERSION.to_string(),
            judge_model: Some(config.judge_model.clone()),
            judge_rationale: Some(format!(
                "Semantic judge skipped because the local run failed: {reason}"
            )),
            critical_mismatches: vec!["Candidate transcript unavailable.".to_string()],
        };
    }

    if config.skip_semantic_judge {
        return QualityReport {
            lexical_accuracy_pct,
            semantic_accuracy_pct: None,
            overall_accuracy_pct: lexical_accuracy_pct,
            wer,
            cer,
            reference_token_count: reference_normalized.tokens.len(),
            candidate_token_count: candidate_normalized.tokens.len(),
            normalization_version: NORMALIZATION_VERSION.to_string(),
            judge_model: None,
            judge_rationale: Some("Semantic judge skipped by --skip-semantic-judge.".to_string()),
            critical_mismatches: Vec::new(),
        };
    }

    let semantic = match get_or_create_semantic_judgment(
        &reference_normalized.text,
        &candidate_normalized.text,
        client,
        config,
        semantic_cache_stats,
    ) {
        Ok(value) => value,
        Err(error) => {
            let input_name = input
                .map(|value| value.file_name.clone())
                .unwrap_or_else(|| result.input_name.clone());
            api_run_errors.push(format!(
                "Semantic judge failed for {} / {} / {}: {error:#}",
                input_name,
                result.model_name,
                result.backend.as_str()
            ));
            return QualityReport {
                lexical_accuracy_pct,
                semantic_accuracy_pct: None,
                overall_accuracy_pct: lexical_accuracy_pct,
                wer,
                cer,
                reference_token_count: reference_normalized.tokens.len(),
                candidate_token_count: candidate_normalized.tokens.len(),
                normalization_version: NORMALIZATION_VERSION.to_string(),
                judge_model: Some(config.judge_model.clone()),
                judge_rationale: Some(format!("Semantic judge failed: {error:#}")),
                critical_mismatches: Vec::new(),
            };
        }
    };

    let overall_accuracy_pct =
        weighted_overall_accuracy(lexical_accuracy_pct, Some(semantic.semantic_accuracy_pct));

    QualityReport {
        lexical_accuracy_pct,
        semantic_accuracy_pct: Some(semantic.semantic_accuracy_pct),
        overall_accuracy_pct,
        wer,
        cer,
        reference_token_count: reference_normalized.tokens.len(),
        candidate_token_count: candidate_normalized.tokens.len(),
        normalization_version: NORMALIZATION_VERSION.to_string(),
        judge_model: Some(semantic.judge_model),
        judge_rationale: Some(semantic.rationale),
        critical_mismatches: semantic.critical_mismatches,
    }
}

fn get_or_create_reference_transcript(
    input: &PreparedInput,
    client: &OpenAiClient,
    config: &EvaluationConfig,
    cache_stats: &mut CacheStats,
) -> Result<ReferenceTranscriptRecord> {
    fs::create_dir_all(reference_cache_dir()).context("failed to create reference cache dir")?;

    let audio_sha256 = sha256_file(&input.path)?;
    let cache_key = hash_string(&format!(
        "{}:{}:{}:{}",
        audio_sha256, config.reference_model, CHUNKING_VERSION, NORMALIZATION_VERSION
    ));
    let cache_path = reference_cache_dir().join(format!("{cache_key}.json"));

    if cache_path.exists() && !config.refresh_reference {
        cache_stats.hits += 1;
        let mut cached: ReferenceTranscriptRecord = read_json(&cache_path)?;
        cached.cache_hit = true;
        cached.cache_path = cache_path.display().to_string();
        cached.input_path = input.path.display().to_string();
        cached.input_name = input.file_name.clone();
        return Ok(cached);
    }

    cache_stats.misses += 1;
    let chunks = split_audio_for_reference(input)?;
    let mut chunk_records = Vec::with_capacity(chunks.len());
    let mut transcript_parts = Vec::with_capacity(chunks.len());
    let mut previous_prompt = None::<String>;

    for chunk in &chunks {
        let prompt = previous_prompt.clone();
        let response = client.transcribe_audio(
            &config.reference_model,
            &chunk.wav_bytes,
            &format!(
                "{}-chunk-{:02}.wav",
                sanitize_file_stem(&input.file_name),
                chunk.index + 1
            ),
            prompt.as_deref(),
        )?;
        let transcript_text = response.text.trim().to_string();
        if transcript_text.is_empty() {
            bail!(
                "reference model returned an empty transcript for {}",
                input.file_name
            );
        }
        previous_prompt = trailing_prompt(&transcript_text);
        transcript_parts.push(transcript_text.clone());
        chunk_records.push(ReferenceChunkRecord {
            index: chunk.index,
            start_ms: chunk.start_ms,
            end_ms: chunk.end_ms,
            wav_size_bytes: chunk.wav_bytes.len(),
            prompt_excerpt: prompt.map(|value| truncate(&value, PROMPT_TAIL_CHARS)),
            transcript_text,
            logprob_count: response.logprob_count,
            average_logprob: response.average_logprob,
            min_logprob: response.min_logprob,
        });
    }

    let transcript_text = join_compact(&transcript_parts);
    let normalized = normalize_for_scoring(&transcript_text);
    let all_logprobs = chunk_records.iter().map(|chunk| chunk.logprob_count).sum();
    let average_logprob = weighted_average_logprob(&chunk_records);
    let min_logprob = chunk_records
        .iter()
        .filter_map(|chunk| chunk.min_logprob)
        .fold(None, |acc, value| {
            Some(acc.map_or(value, |current: f64| current.min(value)))
        });

    let record = ReferenceTranscriptRecord {
        input_path: input.path.display().to_string(),
        input_name: input.file_name.clone(),
        model: config.reference_model.clone(),
        audio_sha256,
        transcript_text,
        normalized_text: normalized.text.clone(),
        reference_token_count: normalized.tokens.len(),
        normalization_version: NORMALIZATION_VERSION.to_string(),
        chunking_version: CHUNKING_VERSION.to_string(),
        chunked: chunk_records.len() > 1,
        chunk_count: chunk_records.len(),
        cache_key,
        cache_path: cache_path.display().to_string(),
        cache_hit: false,
        total_logprob_count: all_logprobs,
        average_logprob,
        min_logprob,
        chunks: chunk_records,
    };

    write_json(&cache_path, &record)?;
    Ok(record)
}

fn get_or_create_semantic_judgment(
    reference_normalized: &str,
    candidate_normalized: &str,
    client: &OpenAiClient,
    config: &EvaluationConfig,
    cache_stats: &mut CacheStats,
) -> Result<SemanticJudgeRecord> {
    fs::create_dir_all(semantic_cache_dir()).context("failed to create semantic cache dir")?;

    let cache_key = hash_string(&format!(
        "{}:{}:{}:{}",
        hash_string(reference_normalized),
        hash_string(candidate_normalized),
        config.judge_model,
        SEMANTIC_RUBRIC_VERSION
    ));
    let cache_path = semantic_cache_dir().join(format!("{cache_key}.json"));

    if cache_path.exists() {
        cache_stats.hits += 1;
        let mut cached: SemanticJudgeRecord = read_json(&cache_path)?;
        cached.cache_hit = true;
        cached.cache_key = cache_key;
        cached.cache_path = cache_path.display().to_string();
        return Ok(cached);
    }

    cache_stats.misses += 1;
    let mut record = client.judge_semantic(
        &config.judge_model,
        reference_normalized,
        candidate_normalized,
    )?;
    record.cache_key = cache_key;
    record.cache_path = cache_path.display().to_string();
    record.cache_hit = false;
    write_json(&cache_path, &record)?;
    Ok(record)
}

fn split_audio_for_reference(input: &PreparedInput) -> Result<Vec<AudioChunk>> {
    if wav_size_bytes(input.audio.len()) <= SAFE_CHUNK_LIMIT_BYTES {
        return Ok(vec![AudioChunk {
            index: 0,
            start_ms: 0,
            end_ms: input.audio_duration_ms,
            wav_bytes: encode_wav_pcm16(&input.audio)?,
        }]);
    }

    let max_samples_per_chunk =
        ((SAFE_CHUNK_LIMIT_BYTES.saturating_sub(WAV_HEADER_BYTES)) / PCM16_BYTES_PER_SAMPLE).max(1);
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;

    while start < input.audio.len() {
        let preferred_end = (start + max_samples_per_chunk).min(input.audio.len());
        let mut end = preferred_end;
        if preferred_end < input.audio.len() {
            if let Some(split_index) =
                find_preferred_split_index(&input.audio, start, preferred_end)
            {
                end = split_index;
            }
        }
        if end <= start {
            end = preferred_end;
        }

        let wav_bytes = encode_wav_pcm16(&input.audio[start..end])?;
        if wav_bytes.len() > SAFE_CHUNK_LIMIT_BYTES {
            bail!("generated chunk exceeded the safe OpenAI upload size");
        }
        chunks.push(AudioChunk {
            index,
            start_ms: duration_ms(start),
            end_ms: duration_ms(end),
            wav_bytes,
        });
        start = end;
        index += 1;
    }

    Ok(chunks)
}

fn find_preferred_split_index(
    samples: &[f32],
    start: usize,
    preferred_end: usize,
) -> Option<usize> {
    let search_window = SILENCE_SEARCH_WINDOW_SECONDS * TARGET_SAMPLE_RATE_HZ as usize;
    let search_start = preferred_end.saturating_sub(search_window).max(start);
    let min_silence_samples = (SILENCE_MIN_MS * TARGET_SAMPLE_RATE_HZ as usize) / 1000;
    let mut run_start = None;
    let mut best_split = None;

    for index in search_start..preferred_end {
        if samples.get(index).copied().unwrap_or(0.0).abs() <= SILENCE_THRESHOLD {
            run_start.get_or_insert(index);
        } else if let Some(start_index) = run_start.take() {
            if index.saturating_sub(start_index) >= min_silence_samples {
                best_split = Some(start_index + ((index - start_index) / 2));
            }
        }
    }

    if let Some(start_index) = run_start {
        if preferred_end.saturating_sub(start_index) >= min_silence_samples {
            best_split = Some(start_index + ((preferred_end - start_index) / 2));
        }
    }

    best_split.filter(|split| *split > start && *split < preferred_end)
}

fn encode_wav_pcm16(samples: &[f32]) -> Result<Vec<u8>> {
    let data_len = samples
        .len()
        .checked_mul(PCM16_BYTES_PER_SAMPLE)
        .context("audio chunk too large to encode as wav")?;
    let riff_len = data_len
        .checked_add(36)
        .context("audio chunk too large to encode as wav")?;

    let data_len_u32 = u32::try_from(data_len).context("wav data too large")?;
    let riff_len_u32 = u32::try_from(riff_len).context("wav file too large")?;

    let mut output = Vec::with_capacity(WAV_HEADER_BYTES + data_len);
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&riff_len_u32.to_le_bytes());
    output.extend_from_slice(b"WAVE");
    output.extend_from_slice(b"fmt ");
    output.extend_from_slice(&16u32.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&TARGET_SAMPLE_RATE_HZ.to_le_bytes());
    let byte_rate = TARGET_SAMPLE_RATE_HZ
        .checked_mul(PCM16_BYTES_PER_SAMPLE as u32)
        .context("byte rate overflow")?;
    output.extend_from_slice(&byte_rate.to_le_bytes());
    output.extend_from_slice(&(PCM16_BYTES_PER_SAMPLE as u16).to_le_bytes());
    output.extend_from_slice(&16u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&data_len_u32.to_le_bytes());

    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let scaled = (clamped * i16::MAX as f32).round() as i16;
        output.extend_from_slice(&scaled.to_le_bytes());
    }

    Ok(output)
}

fn wav_size_bytes(sample_count: usize) -> usize {
    WAV_HEADER_BYTES + (sample_count * PCM16_BYTES_PER_SAMPLE)
}

fn trailing_prompt(transcript: &str) -> Option<String> {
    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .chars()
            .rev()
            .take(PROMPT_TAIL_CHARS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    )
}

fn weighted_average_logprob(chunks: &[ReferenceChunkRecord]) -> Option<f64> {
    let mut total_count = 0usize;
    let mut weighted_sum = 0.0f64;

    for chunk in chunks {
        if let Some(avg) = chunk.average_logprob {
            total_count += chunk.logprob_count;
            weighted_sum += avg * chunk.logprob_count as f64;
        }
    }

    if total_count == 0 {
        None
    } else {
        Some(weighted_sum / total_count as f64)
    }
}

fn normalize_for_scoring(text: &str) -> NormalizedTranscript {
    let lower = text
        .nfkc()
        .flat_map(|ch| match ch {
            '’' | '‘' | '`' => "'".chars().collect::<Vec<_>>(),
            _ => ch.to_lowercase().collect::<Vec<_>>(),
        })
        .collect::<String>();

    let chars = lower.chars().collect::<Vec<_>>();
    let mut cleaned = String::with_capacity(chars.len());
    let mut previous_was_space = true;

    for (index, current) in chars.iter().copied().enumerate() {
        let next = chars.get(index + 1).copied();
        if current.is_alphanumeric() {
            cleaned.push(current);
            previous_was_space = false;
            continue;
        }
        if current == '\'' {
            let prev = cleaned.chars().last();
            if prev.map(is_word_char).unwrap_or(false) && next.map(is_word_char).unwrap_or(false) {
                cleaned.push(current);
                previous_was_space = false;
                continue;
            }
        }
        if !previous_was_space {
            cleaned.push(' ');
            previous_was_space = true;
        }
    }

    let normalized = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let tokens = normalized
        .split_whitespace()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let chars = normalized.chars().collect::<Vec<_>>();
    NormalizedTranscript {
        text: normalized,
        tokens,
        chars,
    }
}

fn is_word_char(value: char) -> bool {
    value.is_alphanumeric()
}

fn compute_lexical_metrics(
    reference: &NormalizedTranscript,
    candidate: &NormalizedTranscript,
) -> (f64, f64, u8) {
    let word_edits = levenshtein_distance(&reference.tokens, &candidate.tokens);
    let char_edits = levenshtein_distance(&reference.chars, &candidate.chars);
    let wer = word_edits as f64 / reference.tokens.len().max(1) as f64;
    let cer = char_edits as f64 / reference.chars.len().max(1) as f64;
    let weighted_error = (0.85 * wer.min(1.0)) + (0.15 * cer.min(1.0));
    let lexical_accuracy = (100.0 * (1.0 - weighted_error)).max(0.0).round() as u8;
    (wer, cer, lexical_accuracy)
}

fn weighted_overall_accuracy(lexical_accuracy_pct: u8, semantic_accuracy_pct: Option<u8>) -> u8 {
    match semantic_accuracy_pct {
        Some(semantic) => ((lexical_accuracy_pct as f64 * 0.75) + (semantic as f64 * 0.25))
            .round()
            .clamp(0.0, 100.0) as u8,
        None => lexical_accuracy_pct,
    }
}

fn levenshtein_distance<T: Eq>(left: &[T], right: &[T]) -> usize {
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }

    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0usize; right.len() + 1];

    for (left_index, left_item) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_item) in right.iter().enumerate() {
            let substitution_cost = usize::from(left_item != right_item);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        previous.clone_from(&current);
    }

    previous[right.len()]
}

fn language_id_to_code(language_id: i32) -> String {
    match language_id {
        0 => "en",
        1 => "zh",
        2 => "de",
        3 => "es",
        4 => "ru",
        5 => "ko",
        6 => "fr",
        7 => "ja",
        8 => "pt",
        9 => "tr",
        10 => "pl",
        11 => "ca",
        12 => "nl",
        13 => "ar",
        14 => "sv",
        15 => "it",
        16 => "id",
        17 => "hi",
        18 => "fi",
        19 => "vi",
        20 => "he",
        21 => "uk",
        22 => "el",
        23 => "ms",
        24 => "cs",
        25 => "ro",
        26 => "da",
        27 => "hu",
        28 => "ta",
        29 => "no",
        30 => "th",
        31 => "ur",
        32 => "hr",
        33 => "bg",
        34 => "lt",
        35 => "la",
        36 => "mi",
        37 => "ml",
        38 => "cy",
        39 => "sk",
        40 => "te",
        41 => "fa",
        42 => "lv",
        43 => "bn",
        44 => "sr",
        45 => "az",
        46 => "sl",
        47 => "kn",
        48 => "et",
        49 => "mk",
        50 => "br",
        51 => "eu",
        52 => "is",
        53 => "hy",
        54 => "ne",
        55 => "mn",
        56 => "bs",
        57 => "kk",
        58 => "sq",
        59 => "sw",
        60 => "gl",
        61 => "mr",
        62 => "pa",
        63 => "si",
        64 => "km",
        65 => "sn",
        66 => "yo",
        67 => "so",
        68 => "af",
        69 => "oc",
        70 => "ka",
        71 => "be",
        72 => "tg",
        73 => "sd",
        74 => "gu",
        75 => "am",
        76 => "yi",
        77 => "lo",
        78 => "uz",
        79 => "fo",
        80 => "ht",
        81 => "ps",
        82 => "tk",
        83 => "nn",
        84 => "mt",
        85 => "sa",
        86 => "lb",
        87 => "my",
        88 => "bo",
        89 => "tl",
        90 => "mg",
        91 => "as",
        92 => "tt",
        93 => "haw",
        94 => "ln",
        95 => "ha",
        96 => "ba",
        97 => "jw",
        98 => "su",
        _ => "auto",
    }
    .to_string()
}

fn available_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as i32)
        .unwrap_or(4)
}

fn collect_machine_metadata(models_dir: &Path) -> Result<RunMetadata> {
    Ok(RunMetadata {
        timestamp_unix_ms: unix_timestamp_ms(),
        hostname: read_command_output("hostname", &[]).unwrap_or_else(|| "unknown".to_string()),
        os_version: read_command_output("sw_vers", &["-productVersion"])
            .unwrap_or_else(|| env::consts::OS.to_string()),
        arch: read_command_output("uname", &["-m"])
            .unwrap_or_else(|| env::consts::ARCH.to_string()),
        cpu_brand: read_command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
            .unwrap_or_else(|| "unknown".to_string()),
        logical_cores: std::thread::available_parallelism()
            .map(|value| value.get() as u64)
            .unwrap_or(0),
        models_dir: models_dir.display().to_string(),
    })
}

fn read_command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn merge_reports(
    cpu_report: BackendRunReport,
    metal_report: Option<BackendRunReport>,
    backend_run_errors: Vec<String>,
    models_dir: &Path,
) -> MergedReport {
    let mut inputs = cpu_report.inputs.clone();
    let mut models = cpu_report.models.clone();
    let mut results = cpu_report.results;
    let mut metadata = cpu_report.run_metadata;

    if let Some(report) = metal_report {
        for input in report.inputs {
            if !inputs.iter().any(|existing| existing.path == input.path) {
                inputs.push(input);
            }
        }
        for model in report.models {
            if !models
                .iter()
                .any(|existing| existing.model_path == model.model_path)
            {
                models.push(model);
            }
        }
        results.extend(report.results);
    }

    metadata.models_dir = models_dir.display().to_string();
    results.sort_by(|left, right| {
        left.input_name
            .cmp(&right.input_name)
            .then(left.model_name.cmp(&right.model_name))
            .then(left.backend.as_str().cmp(right.backend.as_str()))
    });
    inputs.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    models.sort_by(|left, right| left.model_name.cmp(&right.model_name));

    MergedReport {
        run_metadata: metadata,
        inputs,
        models,
        results,
        backend_run_errors,
        reference_transcripts: BTreeMap::new(),
        evaluation_metadata: None,
        api_run_errors: Vec::new(),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    serde_json::to_writer_pretty(file, value)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("failed to parse {}", path.display()))
}

fn print_terminal_report(report: &MergedReport) {
    println!("Whisper Model Benchmark");
    println!(
        "Machine: {} · macOS {} · {} · {} logical cores",
        report.run_metadata.cpu_brand,
        report.run_metadata.os_version,
        report.run_metadata.arch,
        report.run_metadata.logical_cores
    );
    println!("Hostname: {}", report.run_metadata.hostname);
    println!("Models dir: {}", report.run_metadata.models_dir);

    if let Some(metadata) = &report.evaluation_metadata {
        println!(
            "Reference model: {} · Judge model: {} · Semantic judge: {}",
            metadata.reference_model,
            metadata.judge_model,
            if metadata.semantic_judge_enabled {
                "enabled"
            } else {
                "skipped"
            }
        );
    }

    let show_quality = report.evaluation_metadata.is_some();

    for input in &report.inputs {
        println!(
            "\nInput: {} ({})",
            input.file_name,
            format_duration_ms(input.audio_duration_ms)
        );
        if show_quality {
            println!(
                "{:<28} {:<7} {:>11} {:>10} {:>10} {:>8} {:>7} {:<7}",
                "Model", "Backend", "Wall", "Audio", "RTF", "Speed", "Acc", "Status"
            );
            println!("{}", "-".repeat(98));
        } else {
            println!(
                "{:<28} {:<7} {:>11} {:>10} {:>10} {:>8} {:<7}",
                "Model", "Backend", "Wall", "Audio", "RTF", "Speed", "Status"
            );
            println!("{}", "-".repeat(88));
        }

        for result in report
            .results
            .iter()
            .filter(|result| result.input_path == input.path)
        {
            let status = if result.success { "ok" } else { "error" };
            if show_quality {
                println!(
                    "{:<28} {:<7} {:>11} {:>10} {:>10} {:>8} {:>7} {:<7}",
                    truncate(&result.model_name, 28),
                    result.backend.as_str(),
                    result
                        .wall_time_ms
                        .map(format_duration_ms)
                        .unwrap_or_else(|| "-".to_string()),
                    format_duration_ms(result.audio_duration_ms),
                    result
                        .realtime_factor
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "-".to_string()),
                    result
                        .speed_multiplier
                        .map(|value| format!("{value:.2}x"))
                        .unwrap_or_else(|| "-".to_string()),
                    result
                        .quality
                        .as_ref()
                        .map(|quality| format!("{}%", quality.overall_accuracy_pct))
                        .unwrap_or_else(|| "-".to_string()),
                    status,
                );
            } else {
                println!(
                    "{:<28} {:<7} {:>11} {:>10} {:>10} {:>8} {:<7}",
                    truncate(&result.model_name, 28),
                    result.backend.as_str(),
                    result
                        .wall_time_ms
                        .map(format_duration_ms)
                        .unwrap_or_else(|| "-".to_string()),
                    format_duration_ms(result.audio_duration_ms),
                    result
                        .realtime_factor
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "-".to_string()),
                    result
                        .speed_multiplier
                        .map(|value| format!("{value:.2}x"))
                        .unwrap_or_else(|| "-".to_string()),
                    status,
                );
            }
            if let Some(error) = &result.error {
                println!("  error: {}", error.lines().next().unwrap_or(error));
            }
        }
    }

    if !report.backend_run_errors.is_empty() {
        println!("\nBackend errors:");
        for error in &report.backend_run_errors {
            println!("- {error}");
        }
    }

    if !report.api_run_errors.is_empty() {
        println!("\nAPI errors:");
        for error in &report.api_run_errors {
            println!("- {error}");
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn format_duration_ms(value: u64) -> String {
    if value >= 60_000 {
        let total_seconds = value as f64 / 1000.0;
        let minutes = (total_seconds / 60.0).floor() as u64;
        let seconds = total_seconds - (minutes as f64 * 60.0);
        format!("{minutes}m {seconds:.1}s")
    } else {
        format!("{:.2}s", value as f64 / 1000.0)
    }
}

fn duration_ms(sample_count: usize) -> u64 {
    (((sample_count as f64 / TARGET_SAMPLE_RATE_HZ as f64) * 1000.0).round() as u64).max(1)
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn join_compact(parts: &[String]) -> String {
    parts
        .iter()
        .flat_map(|part| part.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_file_stem(file_name: &str) -> String {
    file_name
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '-'
            }
        })
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn hash_string(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hash_bytes(&hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to hash {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hash_bytes(&hasher.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn print_usage() {
    println!(
        "Usage:\n  cargo run --manifest-path tools/model-bench/Cargo.toml -- compare --input <file> [--input <file>] [--input <file>] [--models-dir <dir>] [--json-out <path>] [--timestamps]\n  cargo run --manifest-path tools/model-bench/Cargo.toml -- evaluate --input <file> [--input <file>] [--input <file>] [--models-dir <dir>] [--json-out <path>] [--timestamps] [--openai-model <id>] [--judge-model <id>] [--refresh-reference] [--skip-semantic-judge]\n\nInternal:\n  cargo run --manifest-path tools/model-bench/Cargo.toml -- run-single --backend <cpu|metal> --input <file> [--input <file>] [--input <file>] --json-out <path> [--models-dir <dir>] [--timestamps]"
    );
}

#[derive(Debug, Clone)]
struct ParsedArgs {
    inputs: Vec<PathBuf>,
    models_dir: Option<PathBuf>,
    json_out: Option<PathBuf>,
    timestamps: bool,
    backend: Option<Backend>,
    openai_model: String,
    judge_model: String,
    refresh_reference: bool,
    skip_semantic_judge: bool,
}

#[derive(Debug, Clone)]
struct ModelSpec {
    model_name: String,
    model_path: PathBuf,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct PreparedInput {
    path: PathBuf,
    file_name: String,
    audio_duration_ms: u64,
    audio: Vec<f32>,
}

#[derive(Debug)]
struct DecodedAudio {
    sample_rate_hz: u32,
    channels: u16,
    samples: Vec<f32>,
}

#[derive(Debug)]
struct PreparedAudio {
    samples: Vec<f32>,
}

#[derive(Debug)]
struct InferenceMetrics {
    transcript_text: String,
    timestamps_monotonic: bool,
    speaker_sequence: Vec<String>,
    cold_load_time_ms: u64,
    warm_load_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Backend {
    Cpu,
    Metal,
}

impl Backend {
    fn parse(value: &OsString) -> Result<Self> {
        match value.to_string_lossy().as_ref() {
            "cpu" => Ok(Self::Cpu),
            "metal" => Ok(Self::Metal),
            other => bail!("invalid backend '{other}', expected cpu or metal"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Metal => "metal",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunMetadata {
    timestamp_unix_ms: u64,
    hostname: String,
    os_version: String,
    arch: String,
    cpu_brand: String,
    logical_cores: u64,
    models_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InputSummary {
    path: String,
    file_name: String,
    audio_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelSummary {
    model_name: String,
    model_path: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchmarkRecord {
    backend: Backend,
    model_name: String,
    model_path: String,
    input_path: String,
    input_name: String,
    audio_duration_ms: u64,
    wall_time_ms: Option<u64>,
    realtime_factor: Option<f64>,
    speed_multiplier: Option<f64>,
    transcript_length: Option<usize>,
    transcript_text: Option<String>,
    timestamps_monotonic: Option<bool>,
    speaker_sequence: Vec<String>,
    peak_memory_bytes: Option<u64>,
    cold_load_time_ms: Option<u64>,
    warm_load_time_ms: Option<u64>,
    success: bool,
    error: Option<String>,
    quality: Option<QualityReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QualityReport {
    lexical_accuracy_pct: u8,
    semantic_accuracy_pct: Option<u8>,
    overall_accuracy_pct: u8,
    wer: f64,
    cer: f64,
    reference_token_count: usize,
    candidate_token_count: usize,
    normalization_version: String,
    judge_model: Option<String>,
    judge_rationale: Option<String>,
    critical_mismatches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackendRunReport {
    run_metadata: RunMetadata,
    inputs: Vec<InputSummary>,
    models: Vec<ModelSummary>,
    results: Vec<BenchmarkRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MergedReport {
    run_metadata: RunMetadata,
    inputs: Vec<InputSummary>,
    models: Vec<ModelSummary>,
    results: Vec<BenchmarkRecord>,
    backend_run_errors: Vec<String>,
    reference_transcripts: BTreeMap<String, ReferenceTranscriptRecord>,
    evaluation_metadata: Option<EvaluationMetadata>,
    api_run_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReferenceTranscriptRecord {
    input_path: String,
    input_name: String,
    model: String,
    audio_sha256: String,
    transcript_text: String,
    normalized_text: String,
    reference_token_count: usize,
    normalization_version: String,
    chunking_version: String,
    chunked: bool,
    chunk_count: usize,
    cache_key: String,
    cache_path: String,
    cache_hit: bool,
    total_logprob_count: usize,
    average_logprob: Option<f64>,
    min_logprob: Option<f64>,
    chunks: Vec<ReferenceChunkRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReferenceChunkRecord {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    wav_size_bytes: usize,
    prompt_excerpt: Option<String>,
    transcript_text: String,
    logprob_count: usize,
    average_logprob: Option<f64>,
    min_logprob: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvaluationMetadata {
    reference_model: String,
    judge_model: String,
    normalization_version: String,
    chunking_version: String,
    semantic_rubric_version: String,
    semantic_judge_enabled: bool,
    reference_cache_hits: u64,
    reference_cache_misses: u64,
    semantic_cache_hits: u64,
    semantic_cache_misses: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SemanticJudgeRecord {
    judge_model: String,
    rubric_version: String,
    semantic_accuracy_pct: u8,
    critical_mismatches: Vec<String>,
    rationale: String,
    cache_key: String,
    cache_path: String,
    cache_hit: bool,
}

#[derive(Debug, Clone)]
struct EvaluationConfig {
    reference_model: String,
    judge_model: String,
    refresh_reference: bool,
    skip_semantic_judge: bool,
}

impl EvaluationConfig {
    fn from_args(args: &ParsedArgs) -> Self {
        Self {
            reference_model: args.openai_model.clone(),
            judge_model: args.judge_model.clone(),
            refresh_reference: args.refresh_reference,
            skip_semantic_judge: args.skip_semantic_judge,
        }
    }
}

#[derive(Debug, Default)]
struct EvaluationCacheStats {
    reference: CacheStats,
    semantic: CacheStats,
}

#[derive(Debug, Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
}

#[derive(Debug, Clone)]
struct AudioChunk {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    wav_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct NormalizedTranscript {
    text: String,
    tokens: Vec<String>,
    chars: Vec<char>,
}

impl NormalizedTranscript {
    fn from_cached(text: String, expected_token_count: usize) -> Self {
        let tokens = text
            .split_whitespace()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let token_count = if expected_token_count == 0 {
            tokens.len()
        } else {
            expected_token_count
        };
        let chars = text.chars().collect::<Vec<_>>();
        Self {
            text,
            tokens: if token_count == tokens.len() {
                tokens
            } else {
                tokens
            },
            chars,
        }
    }
}

#[derive(Debug)]
struct OpenAiClient {
    api_key: String,
    http: Client,
}

impl OpenAiClient {
    fn new(api_key: String) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
            .build()
            .context("failed to build OpenAI HTTP client")?;
        Ok(Self { api_key, http })
    }

    fn transcribe_audio(
        &self,
        model: &str,
        wav_bytes: &[u8],
        file_name: &str,
        prompt: Option<&str>,
    ) -> Result<OpenAiTranscriptionResponse> {
        let file_part = multipart::Part::bytes(wav_bytes.to_vec())
            .file_name(file_name.to_string())
            .mime_str("audio/wav")
            .context("failed to build multipart audio part")?;

        let mut form = multipart::Form::new()
            .text("model", model.to_string())
            .text("response_format", "json".to_string())
            .text("include[]", "logprobs".to_string())
            .part("file", file_part);

        if let Some(prompt) = prompt {
            if !prompt.trim().is_empty() {
                form = form.text("prompt", prompt.trim().to_string());
            }
        }

        let response = self
            .http
            .post(format!("{OPENAI_API_BASE}/audio/transcriptions"))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .context("failed to call OpenAI transcription API")?
            .error_for_status()
            .context("OpenAI transcription API returned an error")?;

        let value: Value = response
            .json()
            .context("failed to parse OpenAI transcription response")?;
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if text.is_empty() {
            bail!("OpenAI transcription response did not include transcript text");
        }

        let (logprob_count, average_logprob, min_logprob) =
            summarize_logprobs(value.get("logprobs"));

        Ok(OpenAiTranscriptionResponse {
            text,
            logprob_count,
            average_logprob,
            min_logprob,
        })
    }

    fn judge_semantic(
        &self,
        model: &str,
        reference_normalized: &str,
        candidate_normalized: &str,
    ) -> Result<SemanticJudgeRecord> {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "semantic_accuracy_pct": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 100
                },
                "critical_mismatches": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "rationale": { "type": "string" }
            },
            "required": [
                "semantic_accuracy_pct",
                "critical_mismatches",
                "rationale"
            ]
        });

        let user_prompt = format!(
            "Score the candidate transcript against the reference transcript.\n\n\
Reference transcript:\n{reference_normalized}\n\n\
Candidate transcript:\n{candidate_normalized}\n\n\
Rubric:\n\
- Ignore punctuation, casing, and harmless filler differences.\n\
- Accept small paraphrases when the meaning remains the same.\n\
- Penalize wrong names, numbers, dates, times, amounts, units, negations, and hallucinated content heavily.\n\
- Penalize omissions when meaning-bearing content is missing.\n\
- Use 100 only when the candidate preserves the full meaning of the reference.\n\
- Report concise critical mismatches only when they materially change meaning."
        );

        let body = json!({
            "model": model,
            "input": [
                {
                    "role": "system",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "You evaluate transcription fidelity. Return only the requested JSON object."
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": user_prompt
                        }
                    ]
                }
            ],
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "transcript_semantic_judgment",
                    "strict": true,
                    "schema": schema
                }
            }
        });

        let response = self
            .http
            .post(format!("{OPENAI_API_BASE}/responses"))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("failed to call OpenAI responses API for semantic judgment")?
            .error_for_status()
            .context("OpenAI responses API returned an error")?;

        let value: Value = response
            .json()
            .context("failed to parse OpenAI semantic judgment response")?;
        let parsed = extract_structured_json(&value)
            .context("OpenAI semantic judge response did not include structured JSON output")?;
        let payload: SemanticJudgePayload =
            serde_json::from_value(parsed).context("failed to decode semantic judge payload")?;

        Ok(SemanticJudgeRecord {
            judge_model: model.to_string(),
            rubric_version: SEMANTIC_RUBRIC_VERSION.to_string(),
            semantic_accuracy_pct: payload.semantic_accuracy_pct,
            critical_mismatches: payload.critical_mismatches,
            rationale: payload.rationale,
            cache_key: String::new(),
            cache_path: String::new(),
            cache_hit: false,
        })
    }
}

fn summarize_logprobs(value: Option<&Value>) -> (usize, Option<f64>, Option<f64>) {
    let Some(Value::Array(items)) = value else {
        return (0, None, None);
    };

    let mut count = 0usize;
    let mut sum = 0.0f64;
    let mut min = None::<f64>;
    for item in items {
        if let Some(logprob) = item.get("logprob").and_then(Value::as_f64) {
            count += 1;
            sum += logprob;
            min = Some(min.map_or(logprob, |current| current.min(logprob)));
        }
    }

    if count == 0 {
        (0, None, None)
    } else {
        (count, Some(sum / count as f64), min)
    }
}

fn extract_structured_json(value: &Value) -> Option<Value> {
    if let Some(parsed) = value.get("output_parsed") {
        return Some(parsed.clone());
    }
    if let Some(parsed) = value.pointer("/output/0/content/0/parsed") {
        return Some(parsed.clone());
    }
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return serde_json::from_str(text).ok();
    }
    if let Some(Value::Array(output_items)) = value.get("output") {
        for item in output_items {
            if let Some(Value::Array(contents)) = item.get("content") {
                for content in contents {
                    if let Some(parsed) = content.get("parsed") {
                        return Some(parsed.clone());
                    }
                    if let Some(text) = content.get("text").and_then(Value::as_str) {
                        if let Ok(parsed) = serde_json::from_str(text) {
                            return Some(parsed);
                        }
                    }
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
struct OpenAiTranscriptionResponse {
    text: String,
    logprob_count: usize,
    average_logprob: Option<f64>,
    min_logprob: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SemanticJudgePayload {
    semantic_accuracy_pct: u8,
    critical_mismatches: Vec<String>,
    rationale: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_removes_punctuation_and_keeps_internal_apostrophes() {
        let normalized = normalize_for_scoring("  Héllo, WORLD! It's   Jon’s test. ");
        assert_eq!(normalized.text, "héllo world it's jon's test");
        assert_eq!(
            normalized.tokens,
            vec!["héllo", "world", "it's", "jon's", "test"]
        );
    }

    #[test]
    fn perfect_match_scores_full_accuracy() {
        let reference = normalize_for_scoring("Mixed language test uno dos three");
        let candidate = normalize_for_scoring("Mixed language test uno dos three");
        let (wer, cer, lexical) = compute_lexical_metrics(&reference, &candidate);
        assert_eq!(wer, 0.0);
        assert_eq!(cer, 0.0);
        assert_eq!(lexical, 100);
        assert_eq!(weighted_overall_accuracy(lexical, Some(100)), 100);
    }

    #[test]
    fn split_audio_respects_safe_limit() {
        let max_samples = (SAFE_CHUNK_LIMIT_BYTES - WAV_HEADER_BYTES) / PCM16_BYTES_PER_SAMPLE;
        let input = PreparedInput {
            path: PathBuf::from("/tmp/long.wav"),
            file_name: "long.wav".to_string(),
            audio_duration_ms: duration_ms(max_samples * 2 + 1000),
            audio: vec![0.25; max_samples * 2 + 1000],
        };

        let chunks = split_audio_for_reference(&input).expect("chunks");
        assert!(chunks.len() >= 2);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.wav_bytes.len() <= SAFE_CHUNK_LIMIT_BYTES));
    }

    #[test]
    fn silence_split_prefers_quiet_run() {
        let preferred_end = TARGET_SAMPLE_RATE_HZ as usize * 4;
        let mut samples = vec![0.2; preferred_end + 100];
        let silence_start = preferred_end - (TARGET_SAMPLE_RATE_HZ as usize / 2);
        for sample in &mut samples[silence_start..preferred_end] {
            *sample = 0.0;
        }

        let split = find_preferred_split_index(&samples, 0, preferred_end).expect("split");
        assert!(split >= silence_start);
        assert!(split < preferred_end);
    }
}
