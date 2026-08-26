use std::io::Cursor;
use std::time::Instant;

use html_publication::{PublicationFormat, PublicationSource};
use html_view_core::{RendererInitialConfig, RendererPreparation};

const EPUB_BYTES: &[u8] = include_bytes!("../../../../Robert D. Putnam - Bowling Alone_ Revised and Updated_ The Collapse and Revival of American Community (2020, Simon & Schuster) - libgen.li.epub");

fn main() {
    let started = Instant::now();
    let publication = PublicationSource::from_reader(PublicationFormat::Epub, Cursor::new(EPUB_BYTES)).expect("embedded EPUB should open");
    println!("publication opened in {:?}; first document is {}", started.elapsed(), publication.document_uris[publication.start_index]);
    let prepared = Instant::now();
    RendererPreparation::from_provider_with_nav(publication.provider, publication.document_uris, publication.start_index, None, RendererInitialConfig::default()).expect("first document should prepare");
    println!("first document prepared in {:?}", prepared.elapsed());
}
