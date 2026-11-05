use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Write;

fn cover() -> Vec<u8> {
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(20, 30, image::Rgb([20, 100, 180])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn check(extension: &str, reader: Cursor<Vec<u8>>) {
    let versions = generate_thumbnail_versions_with_pdf_renderer(extension, reader, |_| panic!("image cover must not use PDFium")).unwrap().unwrap();
    let decoded = image::load_from_memory(&versions.high_density).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (600, 900));
    let pixel = decoded.get_pixel(0, 0).0;
    assert!(pixel[0] < 40 && pixel[1] > 80 && pixel[2] > 160);
}

#[test]
fn fb2_declared_binary_cover_becomes_library_thumbnails() {
    let xml = format!(
        r##"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink"><description><title-info><book-title>Book</book-title><coverpage><image l:href="#cover"/></coverpage></title-info></description><body><section><p>Text</p></section></body><binary id="cover" content-type="image/png">{}</binary></FictionBook>"##,
        STANDARD.encode(cover())
    );
    check("fb2", Cursor::new(xml.into_bytes()));
}

#[test]
fn cbz_uses_comicinfo_cover_instead_of_first_page() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("1.png", b"not the cover".to_vec()), ("2.png", cover()), ("ComicInfo.xml", br#"<ComicInfo><Pages><Page Image="1" Type="FrontCover"/></Pages></ComicInfo>"#.to_vec())] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    check("cbz", zip.finish().unwrap());
}
