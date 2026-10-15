//! Linux memory and timing measurements for George and Alba using the reader's Rust TTS dependency.
//! Run a fresh process per model. No Python interpreter, reader window, or audio device.
//!
//! cargo run --release -p reader-speech --example speech_benchmark -- \
//!     BUNDLE_DIRECTORY MODEL_ONNX OUTPUT_JSON [THREADS=4] [WARM_REPEATS=3] [VOICE=george|alba]
use serde_json::{Value, json};
use sherpa_onnx::{GeneratedAudio, GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsModelConfig, OfflineTtsVitsModelConfig, git_sha1, onnxruntime_version, version as sherpa_version};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const PASSAGE: &str = include_str!("../../../tts-demo/passage.txt");
const OPENING: &str = "On 14 October, Dr. Andersson opened the old library at half past eight.";
const GEORGE_SPEAKER: i32 = 26;

fn memory() -> Value {
    let status = fs::read_to_string("/proc/self/status").expect("Linux process memory counters");
    let value = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0);
    json!({ "rss_kib": value("VmRSS:"), "anonymous_rss_kib": value("RssAnon:"), "peak_rss_kib": value("VmHWM:") })
}

fn record(phases: &mut Vec<Value>, phase: &str, started: Instant) {
    let mut sample = memory();
    sample["phase"] = json!(phase);
    sample["elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    println!("{sample}");
    io::stdout().flush().expect("flush phase measurement");
    phases.push(sample);
}

fn cpu_ticks() -> u64 {
    let stat = fs::read_to_string("/proc/self/stat").expect("Linux CPU counters");
    let mut fields = stat.rsplit_once(')').expect("process stat fields").1.split_whitespace().skip(11);
    fields.next().unwrap().parse::<u64>().unwrap() + fields.next().unwrap().parse::<u64>().unwrap()
}

fn generate(model: &OfflineTts, text: &str, config: &GenerationConfig) -> GeneratedAudio {
    let audio = model.generate_with_config(text, config, None::<fn(&[f32], f32) -> bool>).expect("synthesize speech");
    assert!(audio.sample_rate() > 0 && !audio.samples().is_empty(), "speech must be nonempty");
    assert!(audio.samples().iter().all(|sample| sample.is_finite()), "speech must be finite");
    assert!(audio.samples().iter().any(|sample| sample.abs() > 1e-5), "speech must not be silent");
    audio
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 { (values[middle - 1] + values[middle]) / 2.0 } else { values[middle] }
}

fn path_string(root: &Path, name: &str) -> Option<String> {
    Some(root.join(name).to_string_lossy().into_owned())
}

fn main() {
    let args: Vec<_> = env::args().collect();
    assert!(args.len() >= 4, "usage: speech_benchmark BUNDLE MODEL OUTPUT_JSON [THREADS=4] [WARM_REPEATS=3] [VOICE=george|alba]");
    let root = PathBuf::from(&args[1]);
    let model_path = PathBuf::from(&args[2]);
    let output = PathBuf::from(&args[3]);
    let threads = args.get(4).map_or(4, |value| value.parse::<i32>().expect("positive thread count"));
    let repeats = args.get(5).map_or(3, |value| value.parse::<usize>().expect("positive repeat count"));
    let voice = args.get(6).map_or("george", String::as_str);
    assert!(matches!(voice, "george" | "alba"), "voice must be george or alba");
    let speaker_id = if voice == "alba" { 0 } else { GEORGE_SPEAKER };
    assert!(threads > 0 && repeats > 0);
    let ticks_per_second = Command::new("getconf").arg("CLK_TCK").output().expect("getconf CLK_TCK");
    let ticks_per_second: f64 = String::from_utf8(ticks_per_second.stdout).unwrap().trim().parse().unwrap();
    let started = Instant::now();
    let mut phases = Vec::new();
    record(&mut phases, "before_load", started);
    // Share inference settings across models; Alba's noise_scale matches the reader loader.
    let mut model_config = OfflineTtsModelConfig { num_threads: threads, provider: Some("cpu".into()), ..Default::default() };
    if voice == "alba" {
        model_config.vits =
            OfflineTtsVitsModelConfig { model: Some(model_path.to_string_lossy().into_owned()), tokens: path_string(&root, "tokens.txt"), data_dir: path_string(&root, "espeak-ng-data"), noise_scale: 0.75, ..Default::default() };
    } else {
        model_config.kokoro = OfflineTtsKokoroModelConfig {
            model: Some(model_path.to_string_lossy().into_owned()),
            voices: path_string(&root, "voices.bin"),
            tokens: path_string(&root, "tokens.txt"),
            data_dir: path_string(&root, "espeak-ng-data"),
            lexicon: path_string(&root, "lexicon-gb-en.txt"),
            lang: Some("en".into()),
            ..Default::default()
        };
    }
    let config = OfflineTtsConfig { model: model_config, max_num_sentences: 1, silence_scale: 0.2, ..Default::default() };
    let load_started = Instant::now();
    let model = OfflineTts::create(&config).expect("load speech model");
    let load_seconds = load_started.elapsed().as_secs_f64();
    assert!(model.num_speakers() > speaker_id);
    record(&mut phases, "after_load", started);
    let generation = GenerationConfig { sid: speaker_id, speed: 1.0, silence_scale: 0.2, ..Default::default() };
    let opening_started = Instant::now();
    drop(generate(&model, OPENING, &generation));
    let opening_seconds = opening_started.elapsed().as_secs_f64();
    record(&mut phases, "after_opening_sentence", started);
    let mut runs = Vec::new();
    for run in 0..=repeats {
        let cpu_start = cpu_ticks();
        let run_started = Instant::now();
        let audio = generate(&model, PASSAGE.trim(), &generation);
        let wall_seconds = run_started.elapsed().as_secs_f64();
        let cpu_seconds = (cpu_ticks() - cpu_start) as f64 / ticks_per_second;
        let audio_seconds = audio.samples().len() as f64 / f64::from(audio.sample_rate());
        if run == 0 {
            record(&mut phases, "first_native_audio_retained", started);
        }
        // Include the PCM handoff performed by the reader's speech worker.
        let pcm = audio.samples().to_vec();
        if run == 0 {
            record(&mut phases, "native_and_rust_pcm_retained", started);
        }
        drop(audio);
        if run == 0 {
            record(&mut phases, "rust_pcm_retained", started);
        }
        drop(pcm);
        let measurement = json!({ "run": run, "wall_seconds": wall_seconds, "cpu_seconds": cpu_seconds, "audio_seconds": audio_seconds });
        println!("{measurement}");
        io::stdout().flush().expect("flush timing measurement");
        runs.push(measurement);
        record(&mut phases, &format!("after_run_{run}_audio_released"), started);
    }
    let warm = &runs[1..];
    let median_seconds = median(warm.iter().map(|run| run["wall_seconds"].as_f64().unwrap()));
    let median_audio_seconds = median(warm.iter().map(|run| run["audio_seconds"].as_f64().unwrap()));
    let after_synthesis = memory();
    drop(model);
    record(&mut phases, "after_voice_unload", started);
    thread::sleep(Duration::from_secs(2));
    record(&mut phases, "two_seconds_after_unload", started);
    let peak_rss_kib = phases.iter().map(|phase| phase["peak_rss_kib"].as_u64().unwrap()).max().unwrap();
    let cpu_info = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu = cpu_info.lines().find_map(|line| line.strip_prefix("model name")?.split_once(':').map(|(_, value)| value.trim())).unwrap_or("unknown");
    let report = json!({
        "host": "Rust; standalone reader-speech example; statically linked native sherpa-onnx",
        "sherpa_onnx": sherpa_version(), "native_git_sha1": git_sha1(), "native_onnxruntime": onnxruntime_version(),
        "speaker": if voice == "alba" { "en_GB-alba-medium" } else { "bm_george" }, "speaker_id": speaker_id,
        "model": model_path, "model_bytes": fs::metadata(&model_path).unwrap().len(), "threads": threads,
        "cpu": cpu, "passage": PASSAGE.trim(), "silence_scale": 0.2, "max_num_sentences": 1,
        "warm_repeats": repeats, "load_seconds": load_seconds, "opening_sentence_seconds": opening_seconds,
        "median_seconds": median_seconds, "median_audio_seconds": median_audio_seconds,
        "times_realtime": median_audio_seconds / median_seconds,
        "median_cpu_seconds": median(warm.iter().map(|run| run["cpu_seconds"].as_f64().unwrap())),
        "after_synthesis": after_synthesis, "peak_rss_kib": peak_rss_kib, "phases": phases, "runs": runs,
        "memory_metric": "Linux /proc/self/status VmRSS and highest observed VmHWM; fresh executable per model",
        "limitations": ["Standalone speech process; excludes reader UI and audio device", "Background desktop activity was not controlled", "Static Rust-linked and Python wheel native libraries may have different build configurations"],
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create results directory");
    }
    fs::write(&output, serde_json::to_string_pretty(&report).unwrap() + "\n").expect("write Rust benchmark results");
    println!("Results: {}", output.display());
}
