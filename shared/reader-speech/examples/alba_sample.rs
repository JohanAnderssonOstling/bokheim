fn main() {
    let model = reader_speech::load_model().expect("load Alba");
    let speed = std::env::args().nth(2).map(|value| value.parse::<f32>().expect("numeric speech speed")).unwrap_or(1.0);
    let audio = model.generate_with_config("This is Alba reading locally from Rust. The voice keeps its natural sentence pauses.", &reader_speech::narration_config(speed, reader_speech::Pauses::NATURAL), None::<fn(&[f32], f32) -> bool>).expect("generate speech");
    assert!(!audio.samples().is_empty());
    let output = std::env::args().nth(1).unwrap_or_else(|| "alba-sample.wav".into());
    assert!(audio.save(&output));
    println!("Generated {:.2} seconds of Alba speech at {} Hz.", audio.samples().len() as f64 / audio.sample_rate() as f64, audio.sample_rate());
}
