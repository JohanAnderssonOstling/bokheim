use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const PDFIUM_LINUX_X64_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7961/pdfium-linux-x64.tgz";
const PDFIUM_LINUX_X64_SHA256: &str = "019665c8877d46fe65f625f80fd714ab07aac68554b0636acf2a2adf9288adb2";
const PDFIUM_WINDOWS_X64_URL: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7961/pdfium-win-x64.tgz";
const PDFIUM_WINDOWS_X64_SHA256: &str = "88276459349b291c41f10422dad0210f007c04d919c8fa56472b6b7c6406adf4";

struct PdfiumTarget {
    archive_url: &'static str,
    archive_sha256: &'static str,
    archive_library_path: &'static str,
    library_name: &'static str,
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target OS is available");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target architecture is available");
    let target = pdfium_target(&target_os, &target_arch);
    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is available"));
    let library_path = output_directory.join(target.library_name);
    let notices_path = output_directory.join("pdfium-third-party-notices.txt");

    if !library_path.exists() || !notices_path.exists() {
        install_pdfium(&target, &library_path, &notices_path);
    }

    println!("cargo:rustc-env=PDFIUM_EMBEDDED_PATH={}", library_path.display());
    println!("cargo:rustc-env=PDFIUM_BUILD_LIBRARY_PATH={}", library_path.display());
    println!("cargo:rustc-env=PDFIUM_LIBRARY_NAME={}", target.library_name);
    println!("cargo:rustc-env=PDFIUM_NOTICES_PATH={}", notices_path.display());
    println!("cargo:rustc-env=PDFIUM_PACKAGED_PATH={}", env::var("PDFIUM_PACKAGED_PATH").unwrap_or_else(|_| "/app/lib/libpdfium.so".to_owned()));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PDFIUM_ARCHIVE_PATH");
    println!("cargo:rerun-if-env-changed=PDFIUM_PACKAGED_PATH");
}

fn pdfium_target(target_os: &str, target_arch: &str) -> PdfiumTarget {
    match (target_os, target_arch) {
        ("linux", "x86_64") => PdfiumTarget { archive_url: PDFIUM_LINUX_X64_URL, archive_sha256: PDFIUM_LINUX_X64_SHA256, archive_library_path: "lib/libpdfium.so", library_name: "libpdfium.so" },
        ("windows", "x86_64") => PdfiumTarget { archive_url: PDFIUM_WINDOWS_X64_URL, archive_sha256: PDFIUM_WINDOWS_X64_SHA256, archive_library_path: "bin/pdfium.dll", library_name: "pdfium.dll" },
        _ => panic!("PDFium build inputs support Linux x86-64 and Windows x86-64; received {target_os} {target_arch}"),
    }
}

fn install_pdfium(target: &PdfiumTarget, library_path: &Path, notices_path: &Path) {
    let bytes = read_pdfium_archive(target);
    let actual_checksum = hex::encode(Sha256::digest(&bytes));
    assert_eq!(actual_checksum, target.archive_sha256, "embedded PDFium checksum mismatch");
    extract_pdfium(target, library_path, notices_path, &bytes);
}

fn read_pdfium_archive(target: &PdfiumTarget) -> Vec<u8> {
    if let Some(path) = env::var_os("PDFIUM_ARCHIVE_PATH") {
        return fs::read(PathBuf::from(path)).expect("failed to read PDFIUM_ARCHIVE_PATH");
    }

    let response = ureq::get(target.archive_url).call().expect("failed to download embedded PDFium");
    let mut bytes = Vec::new();
    response.into_body().into_reader().read_to_end(&mut bytes).expect("failed to read embedded PDFium archive");
    bytes
}

fn extract_pdfium(target: &PdfiumTarget, library_path: &Path, notices_path: &Path, bytes: &[u8]) {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut library = archive
        .entries()
        .expect("failed to read embedded PDFium archive")
        .find_map(|entry| {
            let entry = entry.ok()?;
            let is_library = entry.path().ok().is_some_and(|path| path == Path::new(target.archive_library_path));
            is_library.then_some(entry)
        })
        .expect("embedded PDFium archive does not contain the expected library");
    let temporary_path = library_path.with_file_name(format!("{}.partial", target.library_name));
    let mut output = fs::File::create(&temporary_path).expect("failed to create embedded PDFium output");
    std::io::copy(&mut library, &mut output).expect("failed to extract embedded PDFium");
    fs::rename(temporary_path, library_path).expect("failed to install embedded PDFium");

    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let notices = archive
        .entries()
        .expect("failed to read embedded PDFium licenses")
        .filter_map(|entry| {
            let mut entry = entry.ok()?;
            let path = entry.path().ok()?.into_owned();
            let is_notice = path == Path::new("LICENSE") || path.starts_with("licenses/");
            if !is_notice || !entry.header().entry_type().is_file() {
                return None;
            }
            let mut text = String::new();
            entry.read_to_string(&mut text).ok()?;
            Some(format!("===== {} =====\n{text}\n", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(notices_path, notices).expect("failed to write embedded PDFium license notices");
}
