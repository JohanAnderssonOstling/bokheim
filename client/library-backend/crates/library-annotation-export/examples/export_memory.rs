//! Reproducible Linux memory probe of the production export path.
//! Build: cargo build --release -p library-annotation-export --example export_memory
//! Prepare outside the measured process: export_memory setup DIR BOOKS ANNOTATIONS
//! Measure fresh processes: export_memory run DIR first|replace|markdown|single
//! Files in DIR are synthetic. No application library or export settings are used.
#[cfg(target_os = "linux")]
mod linux {
    use book_inspection::InspectedBook;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookFormat, BookRecord};
    use content_address::{BookId, ContentHash};
    use library_annotation_export::{export, export_prepared, plan, ExportControl};
    use library_database::{Database, ImportCommit};
    use library_files::AssetStore;
    use library_model::{AnnotationExportConflict, AnnotationExportFormat, AnnotationExportOptions, AnnotationExportOutcome, PublishedStagedBook};
    use serde_json::json;
    use std::{
        env,
        error::Error,
        fs,
        io::{Cursor, Write},
        path::Path,
        time::Instant,
    };
    use sync_common::{LibraryId, ROOT_DIR_ID};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    fn options(folder: &Path) -> AnnotationExportOptions {
        AnnotationExportOptions {
            destination: folder.to_owned(),
            formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest],
            install_readers: false, conflict: AnnotationExportConflict::Replace,
            overrides: vec![],
        }
    }

    fn epub(index: usize, annotations: usize) -> Result<Vec<u8>> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let paragraphs = format!("<p>{}</p>", "A highlighted passage with some context. ".repeat(6)).repeat(annotations);
        for (name, body) in [
        ("mimetype", "application/epub+zip".to_owned()),
        ("META-INF/container.xml", "<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"book.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>".to_owned()),
        ("book.opf", format!("<package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"id\" version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"id\">urn:export-memory:{index}</dc:identifier><dc:title>Book {index:05}</dc:title><dc:language>en</dc:language></metadata><manifest><item id=\"chapter\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>")),
        ("chapter.xhtml", format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Chapter</title></head><body>{paragraphs}</body></html>")),
    ] {
        zip.start_file(name, stored)?;
        zip.write_all(body.as_bytes())?;
    }
        Ok(zip.finish()?.into_inner())
    }

    async fn setup(folder: &Path, count: usize, annotations: usize) -> Result<()> {
        // create_dir prevents accidental reuse or deletion of a real directory.
        fs::create_dir(folder)?;
        let root = folder.join("library");
        fs::create_dir(&root)?;
        let library = LibraryId::new_v4();
        fs::write(folder.join("library-id"), library.to_string())?;
        let db = Database::open(folder.join("metadata.sqlite"))?;
        db.initialize_library_for(library)?;
        for index in 0..count {
            let bytes = epub(index, annotations)?;
            let checksum = ContentHash::from(blake3::hash(&bytes));
            let book = BookId::for_local_content(library.as_bytes(), checksum);
            let file_name = format!("Book {index:05}.epub");
            fs::write(root.join(&file_name), &bytes)?;
            db.commit_import(ImportCommit {
                source_identity: None,
                byte_checksum: Some(checksum),
                parent_id: ROOT_DIR_ID,
                file_name: file_name.clone(),
                format: BookFormat::Epub,
                content_hash: book,
                size_bytes: bytes.len() as u64,
                inspection_version: 1,
                inspection: Some(InspectedBook { metadata: BookRecord { metadata: BookDetails { title: format!("Book {index:05}"), ..Default::default() }, description: String::new() }, pdf: None, audiobook: None, toc: vec![] }),
                published: PublishedStagedBook { name: file_name.clone(), relative_path: format!("/{file_name}"), published: true, fingerprint: Some("fixture".into()) },
                request_thumbnail: false,
                restore_paths: vec![],
            })?;
            let incoming = (0..annotations)
                .map(|number| Annotation {
                    id: format!("memory-{index}-{number}"),
                    content_hash: book,
                    anchor: AnnotationAnchor::epub_publication_cfi(format!("epubcfi(/6/2!/4/{},/1:0,/1:246)", (number + 1) * 2)),
                    exact_text: "A highlighted passage with some context. ".repeat(6),
                    note: "A personal annotation about this passage. ".repeat(3),
                    style: AnnotationStyle::Highlight,
                    color: 0xf6c945,
                    created_at: 1,
                    modified_at: 2,
                    toc_ordinal: None,
                    progress: None,
                    deleted: false,
                })
                .collect::<Vec<_>>();
            db.import_export_annotations(book, &incoming)?;
        }
        let assets = AssetStore::open(root.to_str().ok_or("non-UTF8 fixture path")?)?;
        let report = export(&db, &assets, library, options(&folder.join("exports")), ExportControl::default(), |_| {}).await?;
        ensure_written(&report.results)?;
        fs::write(folder.join("fixture.json"), serde_json::to_vec_pretty(&json!({"books":count, "annotations_per_book":annotations, "quote_bytes":246, "note_bytes":126}))?)?;
        println!("{}", json!({"setup_books":count, "annotations_per_book":annotations, "existing_json_bytes":json_bytes(&folder.join("exports"))?}));
        Ok(())
    }

    fn ensure_written(results: &[library_model::AnnotationExportResult]) -> Result<()> {
        for result in results {
            if !matches!(result.outcome, AnnotationExportOutcome::Written) {
                return Err(format!("export failed: {result:?}").into());
            }
        }
        Ok(())
    }

    fn json_bytes(folder: &Path) -> Result<u64> {
        let mut size = 0;
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "json") {
                size += fs::metadata(path)?.len();
            }
        }
        Ok(size)
    }

    fn memory_kib(key: &str) -> u64 {
        fs::read_to_string("/proc/self/status").ok().and_then(|status| status.lines().find_map(|line| line.strip_prefix(key).and_then(|value| value.split_whitespace().next()).and_then(|value| value.parse().ok()))).unwrap_or(0)
    }

    async fn run(folder: &Path, mode: &str) -> Result<()> {
        let library = fs::read_to_string(folder.join("library-id"))?.trim().parse()?;
        let db = Database::open(folder.join("metadata.sqlite"))?;
        let root = folder.join("library");
        let assets = AssetStore::open(root.to_str().ok_or("non-UTF8 fixture path")?)?;
        let mut options = options(&folder.join("exports"));
        match mode {
            "first" => options.destination = folder.join(format!("first-{}", std::process::id())),
            "markdown" => options.formats = vec![AnnotationExportFormat::Markdown],
            "replace" | "single" => {}
            _ => return Err("mode must be first, replace, markdown or single".into()),
        }
        let before_kib = memory_kib("VmRSS:");
        let start = Instant::now();
        let mut prepared_kib = 0;
        let mut progress = |_: library_model::AnnotationExportProgress| {
            if prepared_kib == 0 {
                prepared_kib = memory_kib("VmRSS:");
            }
        };
        let report = if mode == "single" {
            // Match the automatic worker's scoped snapshot/export path, without
            // timers, startup catchup, receipts or toast/UI overhead.
            let ids = db.annotation_export_book_ids()?;
            let mut books = db.annotation_export_books(&ids[..1])?;
            let plan = plan(library, &books, &options)?;
            export_prepared(&db, &assets, library, options, &mut books, plan, ExportControl::default(), &mut progress).await?
        } else {
            export(&db, &assets, library, options, ExportControl::default(), &mut progress).await?
        };
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        ensure_written(&report.results)?;
        println!(
            "{}",
            json!({
                "mode":mode, "fixture":serde_json::from_slice::<serde_json::Value>(&fs::read(folder.join("fixture.json"))?)?,
                "outputs":report.results.len(), "existing_json_bytes":json_bytes(&folder.join("exports"))?,
                "before_kib":before_kib, "after_preparation_kib":prepared_kib, "peak_kib":memory_kib("VmHWM:"),
                "end_kib":memory_kib("VmRSS:"), "elapsed_ms":elapsed_ms,
            })
        );
        Ok(())
    }

    pub async fn run_cli() -> Result<()> {
        let args = env::args().collect::<Vec<_>>();
        match args.get(1).map(String::as_str) {
            Some("setup") if args.len() == 5 => setup(Path::new(&args[2]), args[3].parse()?, args[4].parse()?).await,
            Some("run") if args.len() == 4 => run(Path::new(&args[2]), &args[3]).await,
            _ => Err("usage: export_memory setup DIR BOOKS ANNOTATIONS | export_memory run DIR first|replace|markdown|single".into()),
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    linux::run_cli().await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This memory probe requires Linux /proc.");
    std::process::exit(1);
}
