//! Generate real card JPEGs and a TSV geometry report from source covers.
//! cargo run -p thumbnail --example card-covers -- OUTPUT_DIR SOURCE.jpg ...
use std::fs;
use std::path::PathBuf;
use thumbnail::{card_thumbnail_geometry, decode_cover_image, encode_card_thumbnail_at_width, encode_thumbnail_at_width, BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(args.next().ok_or("usage: card-covers OUTPUT_DIR SOURCE.jpg ...")?);
    fs::create_dir_all(&output)?;
    println!("key\tsource_width\tsource_height\tcard_width\tcard_height\thorizontal_crop_percent\tvertical_crop_percent\tvertical_stretch_percent");
    for source in args.map(PathBuf::from) {
        let stem = source.file_stem().and_then(|name| name.to_str()).ok_or("cover has no file stem")?;
        let key = stem.strip_suffix("-detail").unwrap_or(stem);
        let bytes = fs::read(&source)?;
        let image = decode_cover_image(&bytes, image::guess_format(&bytes)?)?;
        let geometry = card_thumbnail_geometry(image.width(), image.height(), BROWSE_THUMBNAIL_WIDTH)?;
        fs::write(output.join(format!("{key}-original.jpg")), encode_thumbnail_at_width(&image, BROWSE_THUMBNAIL_WIDTH)?)?;
        for width in [BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH] {
            fs::write(output.join(format!("{key}-card-{width}.jpg")), encode_card_thumbnail_at_width(&image, width)?)?;
        }
        let horizontal_crop = 100.0 * (1.0 - geometry.source_width / f64::from(image.width()));
        let vertical_crop = 100.0 * (1.0 - geometry.source_height / f64::from(image.height()));
        let stretch = 100.0 * (f64::from(geometry.output_height) * geometry.source_width / (f64::from(geometry.output_width) * geometry.source_height) - 1.0);
        println!("{key}\t{}\t{}\t{}\t{}\t{horizontal_crop:.6}\t{vertical_crop:.6}\t{stretch:.6}", image.width(), image.height(), geometry.output_width, geometry.output_height);
    }
    Ok(())
}
