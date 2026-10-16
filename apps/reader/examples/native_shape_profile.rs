//! Measure native shaping without the GPUI line-layout cache or HTML adapter.
//! cargo run --release -p reader-ui --example native_shape_profile
//! Allocation counts cover only the calling thread during `layout_line`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::cell::Cell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use gpui::{FontFallbacks, FontFeatures, FontRun, LineLayout, font, px};

#[derive(Clone, Copy, Default)]
struct Allocations {
    calls: usize,
    bytes: usize,
    reallocations: usize,
}

thread_local! {
    static COUNTS: Cell<Option<Allocations>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn count(bytes: usize, reallocation: bool) {
    let _ = COUNTS.try_with(|counts| {
        if let Some(mut value) = counts.get() {
            value.calls += 1;
            value.bytes += bytes;
            value.reallocations += usize::from(reallocation);
            counts.set(Some(value));
        }
    });
}

// Forward the original allocation contract unchanged. The counter uses only
// const-initialized thread-local cells and cannot allocate recursively.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() {
            count(layout.size(), false);
        }
        result
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() {
            count(layout.size(), false);
        }
        result
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, new_size) };
        if !result.is_null() {
            count(new_size, true);
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn hash_layout(layout: &LineLayout, hash: &mut DefaultHasher) {
    for value in [layout.width, layout.ascent, layout.descent, layout.font_size] {
        f32::from(value).to_bits().hash(hash);
    }
    layout.len.hash(hash);
    layout.runs.len().hash(hash);
    for run in &layout.runs {
        run.font_id.hash(hash);
        for glyph in &run.glyphs {
            glyph.id.hash(hash);
            glyph.index.hash(hash);
            glyph.is_emoji.hash(hash);
            f32::from(glyph.position.x).to_bits().hash(hash);
            f32::from(glyph.position.y).to_bits().hash(hash);
        }
    }
}

fn main() {
    let platform = gpui_platform::current_platform(true);
    let system = platform.text_system();
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Regular.ttf").as_slice()), Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Regular.ttf").as_slice())])
        .expect("bundled fonts");
    let mut serif = font("NV Garamond");
    serif.features = FontFeatures(Arc::new(["ccmp", "rlig", "rclt", "mark", "mkmk", "kern", "liga", "clig", "calt"].map(|tag| (tag.to_owned(), 1)).into()));
    let mut fallback_serif = serif.clone();
    fallback_serif.fallbacks = Some(FontFallbacks::from_fonts(vec!["Raleway".to_owned()]));
    let serif = system.font_id(&serif).expect("serif font");
    let fallback_serif = system.font_id(&fallback_serif).expect("serif with fallback");
    let sans = system.font_id(&font("Raleway")).expect("sans font");
    let prose = "The history of books offers a different perspective on technology, knowledge, and society. An office filled with fine illustrations and thoughtful writing invites careful reading. ";
    for (name, body, mixed_fonts) in [
        ("paragraph", prose.repeat(3), false),
        ("long_run", prose.repeat(24), false),
        ("mixed_scripts", "office a\u{301} 😀 אבג سلام 中文 \u{2003} ".repeat(15), true),
        ("paragraph_separators", format!("{}\u{001c}אבג\u{2028}{}", prose.repeat(2), prose), true),
    ] {
        let serif = if mixed_fonts { fallback_serif } else { serif };
        let texts = (0..65).map(|index| format!("{body}{index:04}")).collect::<Vec<_>>();
        let mut allocations = Allocations::default();
        let mut time_ns = 0;
        let mut glyphs = 0;
        let mut geometry = DefaultHasher::new();
        for (index, text) in texts.iter().enumerate() {
            let split = text.char_indices().nth(text.chars().count() / 2).unwrap().0;
            let runs = if mixed_fonts { vec![FontRun { len: split, font_id: serif }, FontRun { len: text.len() - split, font_id: sans }] } else { vec![FontRun { len: text.len(), font_id: serif }] };
            if index > 0 {
                COUNTS.with(|counts| counts.set(Some(Allocations::default())));
            }
            let started = Instant::now();
            let layout = black_box(system.layout_line(black_box(text), px(19.0), black_box(&runs)));
            let elapsed = started.elapsed().as_nanos();
            let measured = COUNTS.with(|counts| counts.replace(None));
            if let Some(measured) = measured {
                allocations.calls += measured.calls;
                allocations.bytes += measured.bytes;
                allocations.reallocations += measured.reallocations;
                time_ns += elapsed;
                glyphs += layout.runs.iter().map(|run| run.glyphs.len()).sum::<usize>();
                hash_layout(&layout, &mut geometry);
            }
        }
        println!(
            "NATIVE_SHAPE_PROFILE {}",
            serde_json::json!({"case":name,"samples":64,"calls":allocations.calls,"allocated_bytes":allocations.bytes,"reallocations":allocations.reallocations,"time_ms":time_ns as f64 / 1e6,"glyphs":glyphs,"geometry_hash":format!("{:016x}",geometry.finish())})
        );
    }
}
