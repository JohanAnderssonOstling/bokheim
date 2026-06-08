use std::path::PathBuf;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/johan/Hem/Programmering/RustroverProjects/Html-Prototype/TestData/Kaashoek, M. Frans, Saltzer, Jerome H._M. Frans Kaashoek - Principles of computer system design an introduction-Elsevier Science_Morgan Kaufmann (2009_2011)/OEBPS/xhtml/title.html"));

    let out = std::env::args().nth(2).unwrap_or_else(|| "/tmp/test_render.png".to_string());

    println!("Loading: {}", path.display());
    let mut renderer = Html_Prototype::HTMLRenderer::from_source_path(path);
    let png = renderer.render_current_page_to_png(800, 600).expect("render failed");

    let non_white = count_non_white_pixels(&png);
    println!("PNG size: {} bytes, non-white pixels: {}", png.len(), non_white);

    std::fs::write(&out, &png).expect("failed to write PNG");
    println!("Written to {}", out);
}

fn count_non_white_pixels(png: &[u8]) -> usize {
    // PNG IDAT contains RGBA pixels after decoding; check raw bytes for non-0xff runs
    // Simple heuristic: count bytes that are not 0xFF (white = 0xFFFFFFFF in RGBA)
    // This is approximate — a proper check would decode the PNG first.
    // Just count pixels in the raw bytes that differ from 0xFF.
    png.iter().filter(|&&b| b != 0xFF).count()
}
