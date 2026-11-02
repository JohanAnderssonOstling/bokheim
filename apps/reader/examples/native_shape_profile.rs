//! Measure native shaping without the GPUI line-layout cache or HTML adapter.
//! cargo run --release -p reader-ui --example native_shape_profile
//! Allocation counts cover only the calling thread during `layout_line`.
//! Set `NATIVE_SHAPE_TEXT_FILE` to profile UTF-8 text from a file instead of the
//! built-in cases, and `NATIVE_SHAPE_SAMPLES` to change the measured sample count.
//! `NATIVE_SHAPE_HOLD` keeps the text system alive after shaping until stdin
//! receives a line, allowing its idle memory to be sampled independently.
//! `NATIVE_SHAPE_RASTERIZE` also renders each distinct glyph at all subpixel
//! positions, dropping the returned bitmaps before the idle sample. Set
//! `NATIVE_SHAPE_FONT_SIZE` to change the default 19px size.
//! `NATIVE_SHAPE_TEXT_SEQUENCE` accepts a JSON array of UTF-8 paragraphs and
//! measures them in order; `NATIVE_SHAPE_SAMPLES` is the number of full passes.
//! `NATIVE_SHAPE_WINDOW_CACHE=retain` profiles GPUI's per-window layout cache;
//! use `drop` to release that cache before the idle memory sample.
//! Use `uncached` to profile the window API without retaining line layouts.
//! `NATIVE_SHAPE_ADAPTER=1` measures the HTML adapter as well, releasing the
//! previous paragraph before each shape. It emits HTML_SHAPE_PROFILE records.
//! `NATIVE_SHAPE_ADAPTER_RETAIN=1` instead keeps the measured paragraphs' paint
//! resources alive together, to measure the cost of a prepared chapter.
//! `NATIVE_SHAPE_ADAPTER_BATCH` commits an append transaction after this many
//! paragraphs, including commit cost and following-batch refaults in timing.
//! Batch profiling requires retained adapter resources.
//! `NATIVE_SHAPE_ADAPTER_REGISTER=1` also registers each shaped run in the
//! engine's glyph metric store, matching the reader's ownership of advances.
//! `NATIVE_SHAPE_METRIC_READ_PASSES` repeats full and advance-only metric
//! lookups, reporting their time separately from shaping and registration.

use html_engine_model::shaping;
use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashSet;
use std::env;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::hint::black_box;
use std::io::{self, Write};
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

use html::text::{
    FontSlant, GlyphId as HtmlGlyphId, GlyphResourceStore, GlyphShaper, ShapedRun as HtmlShapedRun,
    TextStyleSpan,
};
use html_gpui::backend::GpuiTextCache;

use gpui::{
    FontFallbacks, FontFeatures, FontRun, GlyphId, LineLayout, PlatformTextSystem,
    RenderGlyphParams, ShapedCluster, ShapedGlyph, ShapedRun, TextRun, TextSystem,
    WindowTextSystem, SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, font, point, px,
};

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

#[derive(Default)]
struct RasterProfile {
    allocations: Allocations,
    attempts: usize,
    rendered: usize,
    empty: usize,
    failed: usize,
    bitmap_bytes: usize,
    time_ns: u128,
    pixels: DefaultHasher,
    first_error: Option<String>,
}

fn rasterize_layout(
    system: &dyn PlatformTextSystem,
    layout: &LineLayout,
    seen: &mut HashSet<(usize, u32, bool)>,
    profile: &mut RasterProfile,
) {
    for run in &layout.runs {
        for glyph in &run.glyphs {
            if !seen.insert((run.font_id.0, glyph.id.0, glyph.is_emoji)) {
                continue;
            }
            for x in 0..SUBPIXEL_VARIANTS_X {
                for y in 0..SUBPIXEL_VARIANTS_Y {
                    let params = RenderGlyphParams {
                        font_id: run.font_id,
                        glyph_id: GlyphId(glyph.id.0),
                        font_size: layout.font_size,
                        subpixel_variant: point(x, y),
                        scale_factor: 1.0,
                        is_emoji: glyph.is_emoji,
                        subpixel_rendering: true,
                        dilation: 0,
                        foreground_color: None,
                    };
                    COUNTS.with(|counts| counts.set(Some(Allocations::default())));
                    let started = Instant::now();
                    let image = system.glyph_raster_bounds(&params).and_then(|bounds| {
                        if bounds.size.width.0 == 0 || bounds.size.height.0 == 0 {
                            Ok(None)
                        } else {
                            system.rasterize_glyph(&params, bounds).map(|(size, data)| Some((bounds, size, data)))
                        }
                    });
                    profile.time_ns += started.elapsed().as_nanos();
                    let measured = COUNTS.with(|counts| counts.replace(None)).expect("raster counters");
                    profile.allocations.calls += measured.calls;
                    profile.allocations.bytes += measured.bytes;
                    profile.allocations.reallocations += measured.reallocations;
                    profile.attempts += 1;
                    match image {
                        Ok(Some((bounds, size, data))) => {
                            profile.rendered += 1;
                            profile.bitmap_bytes += data.len();
                            run.font_id.hash(&mut profile.pixels);
                            glyph.id.0.hash(&mut profile.pixels);
                            x.hash(&mut profile.pixels);
                            y.hash(&mut profile.pixels);
                            bounds.origin.x.0.hash(&mut profile.pixels);
                            bounds.origin.y.0.hash(&mut profile.pixels);
                            size.width.0.hash(&mut profile.pixels);
                            size.height.0.hash(&mut profile.pixels);
                            data.hash(&mut profile.pixels);
                        }
                        Ok(None) => profile.empty += 1,
                        Err(error) => {
                            profile.failed += 1;
                            if profile.first_error.is_none() {
                                profile.first_error = Some(format!("{error:#}"));
                            }
                        }
                    }
                }
            }
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
    serif.features = FontFeatures::new(
        [
            "ccmp", "rlig", "rclt", "mark", "mkmk", "kern", "liga", "clig", "calt",
        ]
        .map(|tag| (tag.to_owned(), 1))
        .into(),
    );
    let mut fallback_serif = serif.clone();
    fallback_serif.fallbacks = Some(FontFallbacks::from_fonts(vec!["Raleway".to_owned()]));
    let cache_mode = env::var("NATIVE_SHAPE_WINDOW_CACHE").ok();
    assert!(cache_mode.as_deref().is_none_or(|mode| matches!(mode, "retain" | "drop" | "uncached")), "NATIVE_SHAPE_WINDOW_CACHE must be retain, drop or uncached");
    let cache_fonts = cache_mode.as_ref().map(|_| [serif.clone(), fallback_serif.clone(), font("Raleway")]);
    let serif = system.font_id(&serif).expect("serif font");
    let fallback_serif = system.font_id(&fallback_serif).expect("serif with fallback");
    let sans = system.font_id(&font("Raleway")).expect("sans font");
    let window_cache = cache_mode.as_ref().map(|_| WindowTextSystem::new(Arc::new(TextSystem::new(system.clone()))));
    let mut adapter = env::var_os("NATIVE_SHAPE_ADAPTER").map(|_| {
        assert!(cache_mode.is_none(), "adapter mode cannot use a separate window cache mode");
        GpuiTextCache::new(Arc::new(WindowTextSystem::new(Arc::new(TextSystem::new(system.clone())))))
    });
    let retain_adapter_resources = env::var_os("NATIVE_SHAPE_ADAPTER_RETAIN").is_some();
    assert!(!retain_adapter_resources || adapter.is_some(), "retaining adapter resources requires adapter mode");
    let adapter_batch_size = env::var("NATIVE_SHAPE_ADAPTER_BATCH").ok().map(|value| value.parse::<usize>().expect("adapter batch size must be an integer"));
    assert!(adapter_batch_size.is_none_or(|size| size > 0 && retain_adapter_resources), "batch profiling requires a positive batch size and retained adapter resources");
    let register_adapter_runs = env::var_os("NATIVE_SHAPE_ADAPTER_REGISTER").is_some();
    assert!(
        !register_adapter_runs || adapter.is_some(),
        "registering adapter runs requires adapter mode"
    );
    let metric_read_passes = env::var("NATIVE_SHAPE_METRIC_READ_PASSES").map_or(0, |value| {
        value
            .parse::<usize>()
            .expect("metric read passes must be an integer")
    });
    assert!(
        metric_read_passes == 0 || register_adapter_runs,
        "metric lookups require registered adapter runs"
    );
    let mut adapter_resources = register_adapter_runs.then(|| {
        GlyphResourceStore::new(
            adapter
                .as_ref()
                .expect("adapter text cache")
                .resource_generation(),
        )
    });
    let mut cache_layout_bytes = 0;
    let mut cache_text_bytes = 0;
    let mut cache_entries = 0;
    let prose = "The history of books offers a different perspective on technology, knowledge, and society. An office filled with fine illustrations and thoughtful writing invites careful reading. ";
    let samples = env::var("NATIVE_SHAPE_SAMPLES").map_or(64, |value| {
        value.parse::<usize>().expect("NATIVE_SHAPE_SAMPLES must be a positive integer")
    });
    assert!(samples > 0, "NATIVE_SHAPE_SAMPLES must be positive");
    let font_size = env::var("NATIVE_SHAPE_FONT_SIZE").map_or(19.0, |value| {
        value.parse::<f32>().expect("NATIVE_SHAPE_FONT_SIZE must be a positive number")
    });
    assert!(font_size.is_finite() && font_size > 0.0, "NATIVE_SHAPE_FONT_SIZE must be finite and positive");
    let rasterize = env::var_os("NATIVE_SHAPE_RASTERIZE").is_some();
    assert!(adapter.is_none() || !rasterize, "adapter mode measures geometry without rasterization");
    let sequence = env::var_os("NATIVE_SHAPE_TEXT_SEQUENCE").map(|path| {
        assert!(env::var_os("NATIVE_SHAPE_TEXT_FILE").is_none(), "choose a text file or a text sequence");
        let paragraphs: Vec<String> = serde_json::from_slice(&std::fs::read(path).expect("shaping sequence input")).expect("JSON array of UTF-8 paragraphs");
        assert!(!paragraphs.is_empty(), "shaping sequence must not be empty");
        paragraphs
    });
    let measured_samples = sequence.as_ref().map_or(samples, |paragraphs| samples.checked_mul(paragraphs.len()).expect("shaping sample count overflow"));
    let cases = if sequence.is_some() {
        vec![("sequence", String::new(), false)]
    } else if let Some(path) = env::var_os("NATIVE_SHAPE_TEXT_FILE") {
        vec![("file", std::fs::read_to_string(path).expect("UTF-8 shaping input"), false)]
    } else {
        vec![
            ("paragraph", prose.repeat(3), false),
            ("long_run", prose.repeat(24), false),
            ("mixed_scripts", "office a\u{301} 😀 אבג سلام 中文 \u{2003} ".repeat(15), true),
            ("paragraph_separators", format!("{}\u{001c}אבג\u{2028}{}", prose.repeat(2), prose), true),
        ]
    };
    for (name, body, mixed_fonts) in cases {
        let serif = if mixed_fonts { fallback_serif } else { serif };
        let mut allocations = Allocations::default();
        let mut time_ns = 0;
        let mut batch_commit_ns = 0;
        let mut batches = 0;
        let mut metric_read_ns = 0;
        let mut advance_read_ns = 0;
        let mut glyphs = 0;
        let mut geometry = DefaultHasher::new();
        let mut raster = RasterProfile::default();
        let mut rasterized_glyphs = HashSet::new();
        for index in 0..=measured_samples {
            // Generate inputs outside the measured allocation interval. Keeping
            // every sample would distort process RAM when profiling large text.
            let body = sequence.as_ref().map_or(body.as_str(), |paragraphs| paragraphs[index.saturating_sub(1) % paragraphs.len()].as_str());
            let text = format!("{body}{index:04}");
            let split = text.char_indices().nth(text.chars().count() / 2).unwrap().0;
            let runs = if mixed_fonts { vec![FontRun { len: split, font_id: serif }, FontRun { len: text.len() - split, font_id: sans }] } else { vec![FontRun { len: text.len(), font_id: serif }] };
            let text_runs = cache_fonts.as_ref().map(|fonts| runs.iter().map(|run| {
                let font = if run.font_id == sans { &fonts[2] } else if run.font_id == fallback_serif { &fonts[1] } else { &fonts[0] };
                TextRun { len: run.len, font: font.clone(), ..Default::default() }
            }).collect::<Vec<_>>());
            if let Some(adapter) = &mut adapter {
                // Release warmup geometry before the first measured shape.
                // The default also drops each previous paragraph outside the
                // measured interval; retained mode models a prepared chapter.
                if !retain_adapter_resources || index <= 1 {
                    adapter.reset();
                    if register_adapter_runs {
                        adapter_resources = Some(GlyphResourceStore::new(adapter.resource_generation()));
                    }
                }
                if adapter_batch_size.is_some_and(|size| index <= 1 || (index - 1) % size == 0) {
                    adapter.begin_append_shaping().expect("begin adapter shaping batch");
                }
                let style = shaping::style(font_size, 400, FontSlant::Normal, 0x000000ff, Some("NV Garamond")).and_then(|resolved_style| shaping::span(0..text.len(), resolved_style)).expect("adapter style");
                let request = shaping::run(&text, style, None).expect("adapter shape request");
                let mut registered_glyphs = register_adapter_runs.then(|| vec![HtmlGlyphId::MAX; text.chars().count()]);
                if index > 0 {
                    COUNTS.with(|counts| counts.set(Some(Allocations::default())));
                }
                let started = Instant::now();
                let shaped = if let Some(glyphs) = &mut registered_glyphs {
                    let resources = adapter_resources.as_mut().expect("adapter resource store");
                    resources.shape_run(adapter, black_box(request), glyphs)
                } else {
                    adapter
                        .shape_run(black_box(request))
                        .map(|run| run.map(HtmlShapedRun::into_geometry))
                };
                let shaped = black_box(shaped).expect("adapter shaping").expect("adapter geometry");
                let elapsed = started.elapsed().as_nanos();
                if let Some(measured) = COUNTS.with(|counts| counts.replace(None)) {
                    allocations.calls += measured.calls;
                    allocations.bytes += measured.bytes;
                    allocations.reallocations += measured.reallocations;
                    time_ns += elapsed;
                    glyphs += shaped.advances().len();
                    shaped.advances().len().hash(&mut geometry);
                    for advance in shaped.advances() {
                        advance.to_bits().hash(&mut geometry);
                    }
                    shaped.cluster_boundaries().hash(&mut geometry);
                    for caret in shaped.caret_stops().expect("adapter caret geometry").iter() {
                        caret.to_bits().hash(&mut geometry);
                    }
                    shaped.ascent().to_bits().hash(&mut geometry);
                    if let Some(glyphs) = &registered_glyphs {
                        glyphs.hash(&mut geometry);
                        let metrics = adapter_resources.as_ref().expect("adapter resource store").metrics();
                        let mut hint = 0;
                        for (glyph, character) in glyphs.iter().zip(text.chars()) {
                            let metric = metrics.get_with_hint(*glyph, &mut hint).expect("registered glyph metric");
                            assert_eq!(metric.ch(), character, "registered character changed");
                            metric.ch().hash(&mut geometry);
                            metric.advance().to_bits().hash(&mut geometry);
                            metric.ascent().to_bits().hash(&mut geometry);
                            metric.descent().to_bits().hash(&mut geometry);
                        }
                        if metric_read_passes > 0 {
                            let started = Instant::now();
                            for _ in 0..metric_read_passes {
                                for glyph in glyphs {
                                    black_box(metrics.get_with_hint(black_box(*glyph), &mut hint).expect("registered glyph metric"));
                                }
                            }
                            metric_read_ns += started.elapsed().as_nanos();
                            let started = Instant::now();
                            for _ in 0..metric_read_passes {
                                for glyph in glyphs {
                                    black_box(metrics.get_with_hint(black_box(*glyph), &mut hint).expect("registered glyph metric").advance());
                                }
                            }
                            advance_read_ns += started.elapsed().as_nanos();
                        }
                    }
                }
                if adapter_batch_size.is_some_and(|size| index == 0 || index % size == 0 || index == measured_samples) {
                    if index > 0 {
                        COUNTS.with(|counts| counts.set(Some(Allocations::default())));
                    }
                    let started = Instant::now();
                    adapter.commit_append_shaping();
                    let elapsed = started.elapsed().as_nanos();
                    if let Some(measured) = COUNTS.with(|counts| counts.replace(None)) {
                        allocations.calls += measured.calls;
                        allocations.bytes += measured.bytes;
                        allocations.reallocations += measured.reallocations;
                        time_ns += elapsed;
                        batch_commit_ns += elapsed;
                        batches += 1;
                    }
                }
                continue;
            }
            if index > 0 {
                COUNTS.with(|counts| counts.set(Some(Allocations::default())));
            }
            let started = Instant::now();
            let direct_layout;
            let cached_layout;
            let layout = if let Some(cache) = &window_cache {
                let runs = text_runs.as_ref().expect("cache text runs");
                if cache_mode.as_deref() == Some("uncached") {
                    direct_layout = black_box(cache.layout_line_uncached(black_box(&text), px(font_size), black_box(runs)));
                    &direct_layout
                } else {
                    cached_layout = black_box(cache.layout_line(black_box(&text), px(font_size), black_box(runs), None));
                    cached_layout.as_ref()
                }
            } else {
                direct_layout = black_box(system.layout_line(black_box(&text), px(font_size), black_box(&runs)));
                &direct_layout
            };
            let elapsed = started.elapsed().as_nanos();
            let measured = COUNTS.with(|counts| counts.replace(None));
            if window_cache.is_some() && cache_mode.as_deref() != Some("uncached") {
                // Every generated input has a unique numeric suffix. Count each
                // retained layout once, excluding keys and allocator metadata.
                cache_entries += 1;
                cache_text_bytes += text.len();
                cache_layout_bytes += size_of::<LineLayout>()
                    + layout.runs.capacity() * size_of::<ShapedRun>()
                    + layout.clusters.capacity() * size_of::<ShapedCluster>()
                    + layout.runs.iter().map(|run| run.glyphs.capacity() * size_of::<ShapedGlyph>()).sum::<usize>();
            }
            if let Some(measured) = measured {
                allocations.calls += measured.calls;
                allocations.bytes += measured.bytes;
                allocations.reallocations += measured.reallocations;
                time_ns += elapsed;
                glyphs += layout.runs.iter().map(|run| run.glyphs.len()).sum::<usize>();
                hash_layout(&layout, &mut geometry);
            }
            if rasterize {
                rasterize_layout(system.as_ref(), &layout, &mut rasterized_glyphs, &mut raster);
            }
        }
        let mut profile = serde_json::json!({"case":name,"samples":measured_samples,"calls":allocations.calls,"allocated_bytes":allocations.bytes,"reallocations":allocations.reallocations,"time_ms":time_ns as f64 / 1e6,"geometry_hash":format!("{:016x}",geometry.finish())});
        if adapter.is_some() {
            if let Some(size) = adapter_batch_size {
                profile["batch_size"] = size.into();
                profile["batches"] = batches.into();
                profile["batch_commit_ms"] = (batch_commit_ns as f64 / 1e6).into();
            }
            profile["characters"] = glyphs.into();
            profile["retained_adapter_resources"] = retain_adapter_resources.into();
            profile["registered_adapter_runs"] = register_adapter_runs.into();
            profile["metric_digest_included"] = register_adapter_runs.into();
            profile["metric_read_passes"] = metric_read_passes.into();
            profile["metric_read_ms"] = (metric_read_ns as f64 / 1e6).into();
            profile["advance_read_ms"] = (advance_read_ns as f64 / 1e6).into();
            if register_adapter_runs {
                let resources = adapter_resources.as_ref().expect("adapter resource store");
                profile["registered_glyphs"] = resources.len().into();
                profile["registered_records"] = resources.record_count().into();
            }
            println!("HTML_SHAPE_PROFILE {profile}");
        } else {
            profile["glyphs"] = glyphs.into();
            println!("NATIVE_SHAPE_PROFILE {profile}");
        }
        if rasterize {
            println!("NATIVE_RASTER_PROFILE {}", serde_json::json!({
                "case":name,"unique_glyphs":rasterized_glyphs.len(),"attempts":raster.attempts,
                "rendered":raster.rendered,"empty":raster.empty,"failed":raster.failed,
                "calls":raster.allocations.calls,"allocated_bytes":raster.allocations.bytes,
                "reallocations":raster.allocations.reallocations,"bitmap_bytes":raster.bitmap_bytes,
                "time_ms":raster.time_ns as f64 / 1e6,
                "pixel_hash":format!("{:016x}",raster.pixels.finish()),"first_error":raster.first_error,
                "scope":"distinct glyphs including warmup; bitmap outputs dropped; no GPU upload"
            }));
        }
    }
    if let Some(mode) = cache_mode {
        if mode == "drop" {
            drop(window_cache);
        }
        println!("NATIVE_SHAPE_WINDOW_CACHE {}", serde_json::json!({
            "mode":mode,"entries":cache_entries,"layout_bytes":cache_layout_bytes,
            "text_bytes":cache_text_bytes,"scope":"unique layouts in one cache frame; excludes key and allocator overhead"
        }));
    }
    if env::var_os("NATIVE_SHAPE_HOLD").is_some() {
        println!("NATIVE_SHAPE_IDLE");
        io::stdout().flush().expect("flush shaping results");
        io::stdin().read_line(&mut String::new()).expect("idle profiling input");
    }
    if register_adapter_runs {
        // Allocate diagnostic report storage only after the idle RAM sample.
        let report = adapter_resources.as_ref().expect("adapter resource store").memory_usage_report();
        let bytes = |label| report.entries.iter().filter(|entry| entry.label == label).map(|entry| entry.bytes).sum::<usize>();
        println!("GLYPH_METRIC_STORAGE {}", serde_json::json!({
            "character_bytes": bytes("GlyphMetrics.run.characters"),
            "advance_bytes": bytes("GlyphMetrics.run.advances.shared"),
            "record_storage_bytes": bytes("GlyphMetrics.records"),
            "scope":"last retained case; diagnostic report generated after the idle RAM sample"
        }));
    }
}
