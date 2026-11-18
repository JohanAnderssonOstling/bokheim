//! Standalone allocation profiler for retained legacy and current wire paths.
//! Fixtures/warmup are outside each measurement. Counters include alloc/realloc
//! calls and requested bytes, excluding allocator metadata and input ownership.
//! Epoch tags keep pre-existing fixture deallocations out of peak-live accounting.
//! Run through wire_allocations.py; this is not a unit-test harness.
use book_model::{BookFormat, BookRecord, BookTocDocument, BookTocEntry, ReadingPosition};
use library_replica::{storage, BookAddition, BookFacts, EnrichmentResult, LibraryStateEntry, ReadingPositionState};
use pdf_view_common::{PdfReaderMetadata, PdfStoredPage};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use sync_common::{http, transport, BookId, OpaquePublication, ReplicaId, ReplicaSeq};

static EPOCH: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
struct Meter;
fn physical(layout: Layout) -> (Layout, usize) {
    let align = layout.align().max(std::mem::align_of::<usize>());
    let offset = align;
    (Layout::from_size_align(layout.size() + offset, align).unwrap(), offset)
}
fn record(size: usize) {
    COUNT.fetch_add(1, Relaxed);
    BYTES.fetch_add(size, Relaxed);
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (physical, offset) = physical(layout);
        let base = System.alloc(physical);
        if base.is_null() {
            return base;
        }
        let epoch = ACTIVE.load(Relaxed);
        *(base as *mut usize) = epoch;
        if epoch != 0 {
            record(layout.size());
        }
        base.add(offset)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (physical, offset) = physical(layout);
        let base = ptr.sub(offset);
        let epoch = ACTIVE.load(Relaxed);
        if epoch != 0 && *(base as *const usize) == epoch {
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        System.dealloc(base, physical);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let (physical, offset) = physical(layout);
        let base = ptr.sub(offset);
        let old_epoch = *(base as *const usize);
        let resized = System.realloc(base, physical, new_size + offset);
        if resized.is_null() {
            return resized;
        }
        let epoch = ACTIVE.load(Relaxed);
        *(resized as *mut usize) = epoch;
        if epoch != 0 {
            if old_epoch == epoch {
                LIVE.fetch_sub(layout.size(), Relaxed);
            }
            record(new_size);
        }
        resized.add(offset)
    }
}
#[global_allocator]
static ALLOCATOR: Meter = Meter;

fn sample<R>(f: impl FnOnce() -> R) -> (R, (usize, usize, usize)) {
    COUNT.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    LIVE.store(0, Relaxed);
    PEAK.store(0, Relaxed);
    ACTIVE.store(EPOCH.fetch_add(1, Relaxed) + 1, Relaxed);
    let output = black_box(f());
    ACTIVE.store(0, Relaxed);
    (output, (COUNT.load(Relaxed), BYTES.load(Relaxed), PEAK.load(Relaxed)))
}
fn run<R>(fixture: &str, operation: &str, mut f: impl FnMut() -> R) {
    drop(f());
    let mut rows = Vec::new();
    for _ in 0..7 {
        let (output, metrics) = sample(&mut f);
        rows.push(metrics);
        drop(output);
    }
    rows.sort();
    let (count, bytes, peak) = rows[3];
    println!("{fixture},{operation},{count},{bytes},{peak}");
}
fn main() {
    let id = BookId::from_bytes([7; 32]);
    let position = LibraryStateEntry::ReadingPosition { book_id: id, value: ReadingPositionState { location: ReadingPosition::pdf_page(25), progress: 25.0 } };
    let addition = LibraryStateEntry::BookAdd {
        book_id: id,
        value: BookAddition {
            facts: BookFacts { added_at: 1, format: BookFormat::Epub },
            record: BookRecord { metadata: Default::default(), description: "d".repeat(256 * 1024) },
            navigation: Some(BookTocDocument::entries((0..2500).map(|i| BookTocEntry { title: format!("Chapter {i}: {}", "title ".repeat(6)), target: format!("chapter-{i}.xhtml"), children: vec![] }).collect())),
            pdf: None,
            recording: None,
            enrichment: Some(EnrichmentResult::NoMatch),
        },
    };
    let pdf = LibraryStateEntry::BookContent {
        book_id: id,
        checksum: "00".repeat(32).parse().unwrap(),
        size_bytes: Some(1_000_000),
        pdf: Some(PdfReaderMetadata { version: 1, book_id: id.to_hex(), pages: vec![PdfStoredPage { width: 612.0, height: 792.0, rotation: 0 }; 10_000], annotations: vec![] }),
    };
    let mut pdf_mixed = pdf.clone();
    if let LibraryStateEntry::BookContent { pdf: Some(metadata), .. } = &mut pdf_mixed {
        for (index, page) in metadata.pages.iter_mut().enumerate() {
            page.width += (index % 37) as f32;
            page.height += (index % 53) as f32;
            page.rotation = ((index % 4) * 90) as u16;
        }
    }
    println!("fixture,operation,alloc_calls,allocated_bytes,peak_live_bytes");
    for (name, state) in [("position", position), ("book_toc2500", addition), ("pdf_uniform10000", pdf), ("pdf_mixed10000", pdf_mixed)] {
        let typed = storage::encode(&state).unwrap();
        let legacy = transport::encode_versioned(&state, 33).unwrap();
        assert_eq!(storage::decode(&typed).unwrap(), state);
        assert_eq!(transport::decode_versioned::<LibraryStateEntry>(&legacy, 4 * 1024 * 1024, 33).unwrap(), state);
        eprintln!("{name}: typed_wire_bytes={}, legacy_wire_bytes={}", typed.len(), legacy.len());
        run(name, "state_encode_typed", || storage::encode(black_box(&state)).unwrap());
        run(name, "state_encode_legacy", || {
            state.validate().unwrap();
            transport::encode_versioned(black_box(&state), 33).unwrap()
        });
        run(name, "state_decode_typed", || storage::decode(black_box(&typed)).unwrap());
        run(name, "state_decode_legacy", || {
            let v: LibraryStateEntry = transport::decode_versioned(black_box(&legacy), 4 * 1024 * 1024, 33).unwrap();
            v.validate().unwrap();
            v
        });
        let publication = state.clone().publication(ReplicaId::from_u128(1), 1, ReplicaSeq::new(1).unwrap());
        run(name, "push_prepare_legacy", || transport::representation::<_, OpaquePublication>(black_box(&publication)).unwrap());
        // Prepare ownership outside the measured interval.
        let mut measurements = Vec::new();
        for _ in 0..8 {
            let owned = publication.clone();
            let (output, metrics) = sample(|| black_box(owned).into_opaque().unwrap());
            measurements.push(metrics);
            drop(output);
        }
        measurements.remove(0);
        measurements.sort();
        let (n, b, p) = measurements[3];
        println!("{name},push_prepare_direct,{n},{b},{p}");
        let opaque = publication.clone().into_opaque().unwrap();
        run(name, "http_size_typed", || http::encoded_len(black_box(&opaque)).unwrap());
        run(name, "http_size_legacy", || transport::encoded_len(black_box(&opaque)).unwrap());
        run(name, "http_encode_typed", || http::encode(black_box(&opaque)).unwrap());
        run(name, "http_encode_legacy", || transport::encode(black_box(&opaque)).unwrap());
        let body = http::encode(&opaque).unwrap();
        run(name, "http_decode_typed", || http::decode::<OpaquePublication>(black_box(&body), 4 * 1024 * 1024).unwrap());
        let body = transport::encode(&opaque).unwrap();
        run(name, "http_decode_legacy", || transport::decode::<OpaquePublication>(black_box(&body), 4 * 1024 * 1024).unwrap());
    }
}
