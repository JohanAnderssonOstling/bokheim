//! Measure the native voice separately from reader layout and text normalization.
//! Run a fresh process per thread count so peak resident memory is comparable.
use reader_speech::{Pauses, load_model_with_threads, narration_config};
use std::{env, fs, process::Command, time::Instant};

fn cpu_ticks() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace().skip(11);
    Some(fields.next()?.parse::<u64>().ok()? + fields.next()?.parse::<u64>().ok()?)
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let threads = args.get(1).and_then(|value| value.parse().ok()).unwrap_or(2);
    let repeats = args.get(2).and_then(|value| value.parse::<usize>().ok()).unwrap_or(8).max(1);
    let ticks_per_second = Command::new("getconf").arg("CLK_TCK").output().ok().and_then(|output| String::from_utf8(output.stdout).ok()).and_then(|text| text.trim().parse::<f64>().ok());
    let started = Instant::now();
    let model = load_model_with_threads(threads).expect("load installed Alba voice");
    eprintln!("threads={threads}, model_load_ms={:.0}", started.elapsed().as_secs_f64() * 1000.0);
    let config = narration_config(1.0, Pauses::NATURAL);
    let cases = [
        ("short", "This is Alba reading locally from Rust, with clear words and natural pauses."),
        (
            "medium",
            "The reader follows each passage as the voice moves through the book, keeping the current text visible and preserving its exact position whenever playback is paused. When reading begins again, the voice continues smoothly from the same place.",
        ),
        (
            "long",
            "The evening light settled over the quiet garden as the last visitors made their way home, leaving the familiar sounds of birds and distant traffic behind them while a gentle breeze moved through the trees and carried the scent of fresh rain across the path, reminding everyone that even an ordinary day can hold a moment of unexpected peace before the next chapter of their lives begins.",
        ),
    ];
    println!("threads,case,repeats,wall_ms,cpu_ms,audio_ms,realtime_factor");
    for (name, text) in cases {
        assert!(text.chars().count() <= reader_speech::MAX_CHUNK_CHARS);
        for _ in 0..2 {
            drop(model.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>).expect("warm voice"));
        }
        let cpu_start = cpu_ticks();
        let started = Instant::now();
        let mut audio_seconds = 0.0;
        for _ in 0..repeats {
            let audio = model.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>).expect("synthesize speech");
            audio_seconds += audio.samples().len() as f64 / f64::from(audio.sample_rate());
        }
        let elapsed = started.elapsed().as_secs_f64();
        let cpu_ms = cpu_start.zip(cpu_ticks()).zip(ticks_per_second).map(|((start, end), frequency)| (end - start) as f64 / frequency * 1000.0 / repeats as f64);
        println!(
            "{threads},{name},{repeats},{:.2},{},{:.2},{:.4}",
            elapsed * 1000.0 / repeats as f64,
            cpu_ms.map_or_else(|| "unavailable".into(), |value| format!("{value:.2}")),
            audio_seconds * 1000.0 / repeats as f64,
            elapsed / audio_seconds
        );
    }
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        if let Some(peak) = status.lines().find(|line| line.starts_with("VmHWM:")) {
            eprintln!("{peak}");
        }
    }
}
