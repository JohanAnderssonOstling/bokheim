//! Profile voice/runtime memory independently of the reader and audio device.
//! Keep the voice for 60 idle seconds, then drop it as the worker's idle timer does.
use reader_speech::{Pauses, load_model, narration_config};
use std::{
    env, fs,
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

fn record(phase: &str, started: Instant) {
    let status = fs::read_to_string("/proc/self/status").expect("Linux process memory counters");
    let value = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0);
    println!("{phase},{:.2},{},{},{}", started.elapsed().as_secs_f64(), value("VmRSS:"), value("RssAnon:"), value("VmHWM:"));
    io::stdout().flush().unwrap();
}

fn main() {
    let idle_seconds = env::args().nth(1).and_then(|value| value.parse().ok()).unwrap_or(60);
    let started = Instant::now();
    println!("phase,elapsed_seconds,rss_kib,anonymous_rss_kib,peak_rss_kib");
    record("before_load", started);
    let model = load_model().expect("load installed Alba voice");
    record("after_load", started);
    let config = narration_config(1.0, Pauses::NATURAL);
    let text = "The evening light settled over the quiet garden as the last visitors made their way home, leaving the familiar sounds of birds and distant traffic behind them while a gentle breeze moved through the trees and carried the scent of fresh rain across the path, reminding everyone that even an ordinary day can hold a moment of unexpected peace before the next chapter of their lives begins.";
    for _ in 0..8 {
        drop(model.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>).expect("warm synthesis"));
    }
    let audio = model.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>).expect("synthesize speech");
    record("native_audio_retained", started);
    let pcm = audio.samples().to_vec();
    record("native_and_rust_pcm_retained", started);
    drop(audio);
    record("rust_pcm_retained", started);
    drop(pcm);
    record("audio_released", started);
    thread::sleep(Duration::from_secs(idle_seconds));
    record("idle_before_unload", started);
    drop(model);
    record("after_voice_unload", started);
    thread::sleep(Duration::from_secs(5));
    record("five_seconds_after_unload", started);
}
