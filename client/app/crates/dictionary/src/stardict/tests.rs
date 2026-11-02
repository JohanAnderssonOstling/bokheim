use std::io::{Cursor, Write};

use flate2::{Compress, Compression, FlushCompress, write::GzEncoder};

use super::*;
use crate::Definition;

fn fixture(types: &str, offset64: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let texts = ["A printed volume.", "A tree.", "A house."];
    let words = ["bok", "bok", "hús"];
    let mut index = Vec::new();
    let mut body = Vec::new();
    for (word, text) in words.into_iter().zip(texts) {
        index.extend_from_slice(word.as_bytes());
        index.push(0);
        if offset64 {
            index.extend_from_slice(&(body.len() as u64).to_be_bytes());
        } else {
            index.extend_from_slice(&(body.len() as u32).to_be_bytes());
        }
        index.extend_from_slice(&(text.len() as u32).to_be_bytes());
        body.extend_from_slice(text.as_bytes());
    }
    let mut aliases = "böcker\0".as_bytes().to_vec();
    aliases.extend_from_slice(&0u32.to_be_bytes());
    let ifo = format!("StarDict's dict ifo file\nversion=3.0.0\nbookname=Example\nwordcount=3\nidxfilesize={}\nidxoffsetbits={}\nsametypesequence={types}\nsynwordcount=1\n", index.len(), if offset64 { 64 } else { 32 }).into_bytes();
    (ifo, index, body, aliases)
}

fn dictzip(bytes: &[u8], length: usize) -> Vec<u8> {
    let mut compressor = Compress::new(Compression::default(), false);
    let mut compressed = Vec::new();
    let mut sizes = Vec::new();
    for chunk in bytes.chunks(length) {
        let before = compressed.len();
        compressed.reserve(length * 2 + 128);
        compressor.compress_vec(chunk, &mut compressed, FlushCompress::Full).unwrap();
        sizes.push((compressed.len() - before) as u16);
    }
    compressed.reserve(128);
    compressor.compress_vec(&[], &mut compressed, FlushCompress::Finish).unwrap();
    let mut gzip = vec![0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 3];
    gzip.extend_from_slice(&(10 + sizes.len() as u16 * 2).to_le_bytes());
    gzip.extend_from_slice(b"RA");
    gzip.extend_from_slice(&(6 + sizes.len() as u16 * 2).to_le_bytes());
    gzip.extend_from_slice(&1u16.to_le_bytes());
    gzip.extend_from_slice(&(length as u16).to_le_bytes());
    gzip.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    for size in sizes {
        gzip.extend_from_slice(&size.to_le_bytes());
    }
    gzip.extend_from_slice(&compressed);
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    gzip.extend_from_slice(&crc.sum().to_le_bytes());
    gzip.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    gzip
}

fn source_files(types: &str, offset64: bool) -> Vec<DictionaryFile> {
    let (ifo, index, bytes, aliases) = fixture(types, offset64);
    [("ifo", ifo), ("idx", index), ("dict", bytes), ("syn", aliases)].into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("dictionary.{suffix}"), bytes }).collect()
}

fn lookup(files: &[DictionaryFile], query: &str) -> Result<Vec<Definition>> {
    let get = |name| files.iter().find(|file| file.name == name).unwrap().bytes.as_slice();
    let compressed = files.iter().any(|file| file.name == DICTZIP_FILE);
    let mut index = Cursor::new(get(IDX_FILE));
    let mut synonyms = files.iter().find(|file| file.name == SYN_FILE).map(|file| Cursor::new(file.bytes.as_slice()));
    let mut body = Body::open(Cursor::new(get(if compressed { DICTZIP_FILE } else { DICT_FILE })), compressed)?;
    let dictionary = Dictionary::open(get(IFO_FILE), index.get_ref(), synonyms.as_ref().map(|reader| *reader.get_ref()), body.len())?;
    dictionary.lookup(query, &mut index, synonyms.as_mut(), &mut body)
}

#[test]
fn lookup_keeps_homonyms_accents_and_aliases_for_both_offset_widths() {
    for offset64 in [false, true] {
        let (_, files) = prepare_import(source_files("m", offset64)).unwrap();
        assert_eq!(lookup(&files, " BOK ").unwrap().len(), 2);
        assert_eq!(lookup(&files, "BöCKER").unwrap()[0].text, "A printed volume.");
        assert_eq!(lookup(&files, "HúS").unwrap()[0].text, "A house.");
        assert!(lookup(&files, "BÖCKER").unwrap().is_empty());
        assert!(lookup(&files, "hu\u{301}s").unwrap().is_empty());
        assert!(lookup(&files, "hus").unwrap().is_empty());
    }
}

#[test]
fn rejects_truncated_indexes_bad_aliases_and_out_of_bounds_entries() {
    let (ifo, index, body, mut aliases) = fixture("m", false);
    assert!(Dictionary::open(&ifo, &index[..index.len() - 1], Some(&aliases), body.len() as u64).is_err());
    assert!(Dictionary::open(&ifo, &index, Some(&aliases), 1).is_err());
    assert!(Dictionary::open(&ifo, &index, None, body.len() as u64).is_err());
    aliases["böcker".len() + 1..].copy_from_slice(&3u32.to_be_bytes());
    assert!(Dictionary::open(&ifo, &index, Some(&aliases), body.len() as u64).is_err());
}

#[test]
fn gzip_indexes_are_expanded_once_for_direct_file_access() {
    let mut input = source_files("m", false);
    let file = input.iter_mut().find(|file| file.name.ends_with(".idx")).unwrap();
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&file.bytes).unwrap();
    file.bytes = gzip.finish().unwrap();
    file.name.push_str(".gz");
    let (_, files) = prepare_import(input).unwrap();
    assert_eq!(lookup(&files, "böcker").unwrap()[0].text, "A printed volume.");
    assert!(!files.iter().any(|file| file.name.ends_with(".idx.gz")));
}

#[test]
fn definitions_are_plain_text_and_preserve_literal_text_fields() {
    let html = "<div>Book &amp; paper</div><script>bad()</script><style>bad</style><br>Second";
    assert_eq!(text::definition(html.as_bytes(), "h").unwrap(), "Book & paper\nSecond");
    assert_eq!(text::definition(b"mMeaning\0tIPA\0", "").unwrap(), "Meaning\nIPA");
    assert!(text::definition(b"mtruncated", "").is_err());
    assert_eq!(text::definition(b"Meaning\0IPA", "mt").unwrap(), "Meaning\nIPA");
    assert_eq!(text::definition(b"<literal> & text\nNext", "m").unwrap(), "<literal> & text\nNext");
    assert_eq!(text::definition(b"<b>House</b>\0<literal> & text\nNext", "hm").unwrap(), "House\n<literal> & text\nNext");
    assert_eq!(text::definition(b"P\0\0\0\x03rawmMeaning\0", "").unwrap(), "Meaning");
}

#[test]
fn dictzip_reads_ranges_crossing_chunk_boundaries_and_rejects_bad_sizes() {
    let bytes = b"abcdefghij0123456789ABCDEFGHIJ";
    let mut gzip = dictzip(bytes, 10);
    let mut body = Body::open(Cursor::new(gzip.clone()), true).unwrap();
    assert_eq!(body.read_entry(8, 17).unwrap(), &bytes[8..25]);
    assert_eq!(body.read_entry(20, 10).unwrap(), &bytes[20..]);
    assert!(body.read_entry(29, 2).is_err());
    let end = gzip.len();
    gzip[end - 4..].copy_from_slice(&100u32.to_le_bytes());
    assert!(Body::open(Cursor::new(gzip), true).is_err());
}

#[test]
fn imports_original_components_without_using_archive_paths() {
    let mut input = source_files("m", false);
    for file in &mut input {
        file.name = format!("../../{}", file.name);
    }
    let (metadata, files) = prepare_import(input).unwrap();
    assert_eq!(metadata.name, "Example");
    assert!(files.iter().all(|file| file.name.starts_with("dictionary.") && !file.name.contains('/')));
    assert!(prepare_import(vec![DictionaryFile { name: "example.ifo".into(), bytes: b"bad".to_vec() }]).is_err());
}

#[test]
fn preserves_license_and_attribution_files_with_the_dictionary() {
    let mut input = source_files("m", false);
    input.push(DictionaryFile { name: "LICENSE".into(), bytes: b"CC-BY-SA-4.0".to_vec() });
    input.push(DictionaryFile { name: "ATTRIBUTION".into(), bytes: b"Wiktionary contributors".to_vec() });
    let (metadata, files) = prepare_import(input).unwrap();
    assert!(metadata.description.contains("Wiktionary contributors"));
    assert!(metadata.description.contains("CC-BY-SA-4.0"));
    assert!(files.iter().any(|file| file.name == "dictionary.license.txt"));
    assert!(files.iter().any(|file| file.name == "dictionary.attribution.txt"));
    assert_eq!(prepare_import(files).unwrap().0, metadata);
}

#[test]
fn imports_nested_zip_with_original_dictzip_and_rejects_corrupt_source_data() {
    let (ifo, index, bytes, aliases) = fixture("m", false);
    let compressed = dictzip(&bytes, 10);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (suffix, bytes) in [("ifo", ifo.clone()), ("idx", index.clone()), ("dict.dz", compressed.clone()), ("syn", aliases.clone())] {
        archive.start_file(format!("folder/example.{suffix}"), zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    let zip = archive.finish().unwrap().into_inner();
    let (_, files) = prepare_import(vec![DictionaryFile { name: "dictionary.zip".into(), bytes: zip }]).unwrap();
    assert_eq!(files.len(), 4);
    assert_eq!(files.iter().find(|file| file.name == DICTZIP_FILE).unwrap().bytes, compressed);
    assert_eq!(lookup(&files, "bok").unwrap()[0].text, "A printed volume.");
    let mut corrupt = compressed;
    let end = corrupt.len();
    corrupt[end - 8] ^= 1;
    let input = [("ifo", ifo), ("idx", index), ("dict.dz", corrupt), ("syn", aliases)].into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("example.{suffix}"), bytes }).collect();
    assert!(prepare_import(input).unwrap_err().contains("Damaged dictzip"));
}

#[test]
fn imports_preserve_all_original_components_and_can_be_reimported() {
    for offset64 in [false, true] {
        let input = source_files("h", offset64);
        let (metadata, files) = prepare_import(input.clone()).unwrap();
        assert_eq!(files.len(), input.len());
        assert!(!files.iter().any(|file| file.name.ends_with(".search") || file.name.ends_with(".json") || file.name.ends_with(".data")));
        for original in &input {
            assert_eq!(files.iter().find(|file| file.name == original.name).unwrap().bytes, original.bytes);
        }
        let (reopened, copies) = prepare_import(files.clone()).unwrap();
        assert_eq!(reopened, metadata);
        for (original, copy) in files.iter().zip(copies) {
            assert_eq!(original.name, copy.name);
            assert_eq!(original.bytes, copy.bytes);
        }
    }
}

#[test]
fn imports_reject_missing_components_and_invalid_original_indexes() {
    let (_, files) = prepare_import(source_files("m", false)).unwrap();
    for name in [IDX_FILE, DICT_FILE, IFO_FILE, SYN_FILE] {
        let mut missing = files.clone();
        missing.retain(|file| file.name != name);
        assert!(prepare_import(missing).is_err(), "must require {name}");
    }
    for name in [IDX_FILE, SYN_FILE] {
        let mut broken = files.clone();
        let bytes = &mut broken.iter_mut().find(|file| file.name == name).unwrap().bytes;
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(prepare_import(broken).is_err());
    }
}

#[test]
fn direct_dictzip_lookup_supports_large_entries_and_reads_in_any_order() {
    let values = ["<b>First</b> &amp; café".repeat(2000), "<p>A long definition.</p>".repeat(10_000), "<i>Last</i>".into()];
    let mut index = Vec::new();
    let mut body = Vec::new();
    for (word, value) in ["first", "large", "last"].into_iter().zip(&values) {
        index.extend_from_slice(word.as_bytes());
        index.push(0);
        index.extend_from_slice(&(body.len() as u32).to_be_bytes());
        index.extend_from_slice(&(value.len() as u32).to_be_bytes());
        body.extend_from_slice(value.as_bytes());
    }
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=Blocks\nwordcount=3\nidxfilesize={}\nsametypesequence=h\n", index.len());
    let input = [("ifo", ifo.into_bytes()), ("idx", index), ("dict.dz", dictzip(&body, 60_000))].into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("blocks.{suffix}"), bytes }).collect();
    let (_, files) = prepare_import(input).unwrap();
    let get = |name| files.iter().find(|file| file.name == name).unwrap().bytes.as_slice();
    let mut index = Cursor::new(get(IDX_FILE));
    let mut body = Body::open(Cursor::new(get(DICTZIP_FILE)), true).unwrap();
    let dictionary = Dictionary::open(get(IFO_FILE), index.get_ref(), None, body.len()).unwrap();
    for (query, expected) in [("last", 2), ("first", 0), ("large", 1), ("last", 2)] {
        assert_eq!(dictionary.lookup(query, &mut index, None, &mut body).unwrap()[0].text, text::html_text(&values[expected]));
    }
}

#[test]
fn cached_offsets_only_read_small_ranges_of_original_indexes() {
    use std::cell::Cell;
    use std::io::{Read, Seek, SeekFrom};
    use std::rc::Rc;
    struct Metered {
        cursor: Cursor<Vec<u8>>,
        read: Rc<Cell<usize>>,
    }
    impl Read for Metered {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            let count = self.cursor.read(output)?;
            self.read.set(self.read.get() + count);
            Ok(count)
        }
    }
    impl Seek for Metered {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.cursor.seek(position)
        }
    }
    let mut raw = Vec::new();
    for word in 0..50_000 {
        raw.extend_from_slice(format!("word-{word:06}\0").as_bytes());
        raw.extend_from_slice(&0u32.to_be_bytes());
        raw.extend_from_slice(&7u32.to_be_bytes());
    }
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=Large\nwordcount=50000\nidxfilesize={}\nsametypesequence=m\nidxlang=de\n", raw.len());
    let dictionary = Dictionary::open(ifo.as_bytes(), &raw, None, 7).unwrap();
    let read = Rc::new(Cell::new(0));
    let mut index = Metered { cursor: Cursor::new(raw), read: read.clone() };
    let mut body = Body::open(Metered { cursor: Cursor::new(b"Meaning".to_vec()), read: read.clone() }, false).unwrap();
    assert_eq!(dictionary.lookup("WORD-049999", &mut index, None, &mut body).unwrap()[0].text, "Meaning");
    assert!(dictionary.lookup("not-found", &mut index, None, &mut body).unwrap().is_empty());
    assert!(read.get() < 4096, "lookup must not read whole files: {} bytes", read.get());
}

#[test]
fn only_matching_definitions_are_parsed() {
    let mut input = source_files("", false);
    // The source index is valid, but these definition fields are malformed.
    let file = input.iter_mut().find(|file| file.name == DICT_FILE).unwrap();
    file.bytes.fill(b'm');
    let (_, files) = prepare_import(input).unwrap();
    assert!(lookup(&files, "absent").unwrap().is_empty());
    assert!(lookup(&files, "bok").is_err());
}

#[test]
fn native_index_order_matches_ascii_case_without_unicode_normalization() {
    let words = ["APPLE", "Apple", "apple", "hús", "Äpfel", "Öl", "äpfel", "öl"];
    let mut index = Vec::new();
    for word in words {
        index.extend_from_slice(word.as_bytes());
        index.push(0);
        index.extend_from_slice(&0u32.to_be_bytes());
        index.extend_from_slice(&7u32.to_be_bytes());
    }
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=Unicode\nwordcount={}\nidxfilesize={}\nsametypesequence=m\n", words.len(), index.len());
    let dictionary = Dictionary::open(ifo.as_bytes(), &index, None, 7).unwrap();
    let mut index = Cursor::new(index);
    let mut body = Body::open(Cursor::new(b"Meaning"), false).unwrap();
    for (query, expected) in [("aPpLe", vec!["APPLE", "Apple", "apple"]), ("ÖL", vec!["Öl"]), ("öl", vec!["öl"]), ("ÄPFEL", vec!["Äpfel"]), ("äpfel", vec!["äpfel"]), ("HúS", vec!["hús"]), ("A\u{308}PFEL", vec![]), ("hu\u{301}s", vec![])]
    {
        let hits = dictionary.lookup(query, &mut index, None, &mut body).unwrap();
        assert_eq!(hits.iter().map(|hit| hit.word.as_str()).collect::<Vec<_>>(), expected);
    }
}

#[test]
fn rejects_unsorted_headword_and_alias_indexes() {
    for name in [IDX_FILE, SYN_FILE] {
        let mut files = source_files("m", false);
        let bytes = &mut files.iter_mut().find(|file| file.name == name).unwrap().bytes;
        let original_size = bytes.len();
        let record_length = bytes.iter().position(|byte| *byte == 0).unwrap() + 1 + if name == IDX_FILE { 8 } else { 4 };
        let mut earlier = bytes[..record_length].to_vec();
        earlier[0] = b'a';
        bytes.extend(earlier);
        let new_size = bytes.len();
        let metadata = files.iter_mut().find(|file| file.name == IFO_FILE).unwrap();
        let mut ifo = String::from_utf8(metadata.bytes.clone()).unwrap();
        if name == IDX_FILE {
            ifo = ifo.replace("wordcount=3\n", "wordcount=4\n").replace(&format!("idxfilesize={original_size}\n"), &format!("idxfilesize={new_size}\n"));
        } else {
            ifo = ifo.replace("synwordcount=1\n", "synwordcount=2\n");
        }
        metadata.bytes = ifo.into_bytes();
        assert!(prepare_import(files).unwrap_err().contains("StarDict order"));
    }
}

#[test]
fn aliases_are_deduplicated_and_alternatives_remain_bounded() {
    let mut index = Vec::new();
    for _ in 0..40 {
        index.extend_from_slice(b"word\0");
        index.extend_from_slice(&0u32.to_be_bytes());
        index.extend_from_slice(&7u32.to_be_bytes());
    }
    let mut synonyms = Vec::new();
    for entry in std::iter::repeat_n(0u32, 100).chain(1..40) {
        synonyms.extend_from_slice(b"alias\0");
        synonyms.extend_from_slice(&entry.to_be_bytes());
    }
    let ifo = format!("StarDict's dict ifo file\nversion=2.4.2\nbookname=Homonyms\nwordcount=40\nsynwordcount=139\nidxfilesize={}\nsametypesequence=m\n", index.len());
    let dictionary = Dictionary::open(ifo.as_bytes(), &index, Some(&synonyms), 7).unwrap();
    let mut body = Body::open(Cursor::new(b"Meaning"), false).unwrap();
    for query in ["WORD", "ALIAS"] {
        assert_eq!(dictionary.lookup(query, &mut Cursor::new(&index), Some(&mut Cursor::new(&synonyms)), &mut body).unwrap().len(), 32);
    }
}
