//! Profile the real native speech worker through playback, pause and closing.
//! BOKHEIM_TTS_MODEL_DIR=... cargo run --release -p reader-speech --example worker_memory
use reader_speech::{DesktopSpeech, SpeechEvent};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

fn record(phase: &str, started: Instant) -> u64 {
    let status = fs::read_to_string("/proc/self/status").expect("Linux process counters");
    let value = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0);
    let rss = value("VmRSS:");
    println!("{phase},{:.3},{rss},{},{}", started.elapsed().as_secs_f64(), value("RssAnon:"), value("VmSwap:"));
    rss
}

fn main() {
    let started = Instant::now();
    println!("phase,elapsed_seconds,rss_kib,anonymous_rss_kib,swap_kib");
    record("before_worker", started);
    for cycle in 0..2 {
        let speech = DesktopSpeech::new().expect("create native worker endpoint");
        let events = speech.events();
        speech.replace(vec![include_str!("../../../tts-demo/passage.txt").to_owned(); 20]);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match events.try_recv() {
                Ok(SpeechEvent::Started { .. }) => break,
                Ok(SpeechEvent::Failed { message, .. }) => panic!("Native synthesis failed: {message}"),
                _ => {}
            }
            assert!(Instant::now() < deadline, "native playback did not start");
            thread::sleep(Duration::from_millis(25));
        }
        record(&format!("cycle_{cycle}_playing"), started);
        thread::sleep(Duration::from_secs(5));
        record(&format!("cycle_{cycle}_warmed"), started);
        speech.pause();
        record(&format!("cycle_{cycle}_paused"), started);
        drop(speech);
        // This receiver is deliberately retained: the listener must not keep
        // the former endpoint's device or model alive after it is closed.
        for sample in 0..20 {
            thread::sleep(Duration::from_millis(250));
            record(&format!("cycle_{cycle}_closed_{sample}"), started);
        }
        drop(events);
    }
    record("finished", started);
}
