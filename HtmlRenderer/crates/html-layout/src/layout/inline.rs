// Inline layout for text and images.
// `layout_runs` now coordinates an explicit pipeline instead of mixing all phases in one loop.
// Call flow: `layout_runs` -> `build_inline_tokens` -> `break_lines`
// -> `emit_lines`; emission uses `measure_line` -> `write_line_fragments`.
// Measurement uses `line_metrics_from_tokens`, `calculate_line_metrics_in_doc`,
// `compute_justification`, `compute_baseline`, and `vertical_align_offset`.
// Token building uses `effective_vertical_align` and `break_kind`.

use crate::document::{Float, GlyphAdvanceRun, GlyphId, GlyphMetrics, GlyphOffsetRun, ImageFragment, InlineKind, InlineRun, Line, OverflowWrap, TabSizeKind, TextAlign, VerticalAlignValue, WhiteSpace, WordBreak};
use crate::layout::{FloatSide, LayoutContext};
use kurbo::{Point, Size};
use std::ops::Range;
use std::time::Instant;

/// When true, justified text uses the Knuth–Plass optimal line breaker instead
/// of the greedy first-fit breaker. On by default; set to false to A/B the two.
/// Promote to a runtime flag on `LayoutContext` if it needs to be user-toggled.
const USE_KNUTH_PLASS: bool = true;

/// Stretch/shrink tolerance handed to the Knuth–Plass solver. Higher values let
/// it accept looser lines (and fall back to greedy less often).
const KP_TOLERANCE: f64 = 8.0;

/// Test-only override so a single build can benchmark both breakers: 0 forces
/// greedy, 1 forces Knuth–Plass, -1 (and every release build) defers to the
/// `USE_KNUTH_PLASS` const.
#[cfg(test)]
pub(crate) static KP_TEST_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

fn knuth_enabled() -> bool {
    #[cfg(test)]
    {
        match KP_TEST_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
            0 => return false,
            1 => return true,
            _ => {}
        }
    }
    USE_KNUTH_PLASS
}

/// Computes the extra word spacing for a justified line.
///
/// This is only used for pure text lines after line breaking has already chosen
/// the glyph span for the line.
fn compute_justification(
    doc: &LayoutContext, metrics: &GlyphMetrics, glyph_start: u32, glyph_end: u32, content_width: f64, available_width: f64, is_last_line: bool, text_align: TextAlign, disable_justify: bool, force_justify: bool,
) -> (f64, f64) {
    if is_last_line || disable_justify || !(text_align == TextAlign::Justify || (force_justify && text_align == TextAlign::Left)) {
        return (0.0, 0.0);
    }

    // `content_width` is the measured line width including letter/word spacing
    // (the sum of token advances), so the slack we distribute matches what the
    // line will actually occupy. Summing raw glyph advances here instead would
    // overestimate the slack whenever letter/word-spacing is set and push the
    // justified line past the column edge.
    let extra_space = available_width - content_width;
    if extra_space <= 0.0 {
        return (0.0, 0.0);
    }

    let space_count = (glyph_start..glyph_end).filter(|&i| metrics.get(doc.glyph_at(i as usize).unwrap_or_default()).ch() == ' ').count();
    if space_count == 0 {
        return (0.0, 0.0);
    }
    (extra_space / space_count as f64, 0.0)
}

/// Derives a final line box from ascent, descent, and a minimum height.
///
/// Callers use this both for raw font metrics and for the adjusted metrics after
/// vertical-align offsets have been applied.
fn compute_baseline(max_ascent: f64, max_descent: f64, min_height: f64) -> (f64, f64) {
    let content_height = max_ascent + max_descent;
    let line_height = content_height.max(min_height);
    let baseline = if content_height > 0.0 { max_ascent + (line_height - content_height).max(0.0) / 2.0 } else { line_height * 0.8 };
    (line_height, baseline)
}

/// Resolves the indent to apply to the current line.
///
/// This keeps the first-line and hanging-indent policy out of the main line
/// breaking loop.
fn indent_for_line(text_indent: f64, hanging: bool, is_first_line: bool) -> f64 {
    if hanging && is_first_line {
        0.0
    } else if hanging || is_first_line {
        text_indent
    } else {
        0.0
    }
}

/// Finds the effective inline `vertical-align` for a box.
///
/// The search walks inline ancestors until a non-inline ancestor resets the
/// context, matching how nested inline vertical alignment is inherited here.
fn effective_vertical_align(doc: &LayoutContext, box_idx: usize) -> VerticalAlignValue {
    let style = doc.style(box_idx);
    if style.vertical_align() != VerticalAlignValue::Baseline {
        return style.vertical_align();
    }

    let mut current = doc.get_parent(box_idx);
    while let Some(ancestor_idx) = current {
        let ancestor_style = doc.style(ancestor_idx);
        if ancestor_style.display() != crate::document::Display::Inline {
            break;
        }
        if ancestor_style.vertical_align() != VerticalAlignValue::Baseline {
            return ancestor_style.vertical_align();
        }
        current = doc.get_parent(ancestor_idx);
    }
    VerticalAlignValue::Baseline
}

struct InlineToken {
    kind: InlineTokenKind,
    width: f64,
    tab_advance: Option<TabAdvance>,
    ascent: f64,
    descent: f64,
    line_height: f64,
    font_size: f32,
    vertical_align: VerticalAlignValue,
    break_kind: BreakKind,
    white_space: WhiteSpace,
    wrap: TokenWrap,
}

#[derive(Clone, Copy)]
struct TabAdvance {
    interval: f64,
    min_advance: f64,
}

impl InlineToken {
    #[inline]
    fn advance_at(&self, position_from_block_start: f64) -> f64 {
        let Some(tab) = self.tab_advance else { return self.width };
        if tab.interval <= 0.0 {
            return 0.0;
        }
        let remainder = position_from_block_start.rem_euclid(tab.interval);
        let mut advance = if remainder <= f64::EPSILON { tab.interval } else { tab.interval - remainder };
        if advance < tab.min_advance {
            advance += tab.interval;
        }
        advance
    }
}

#[derive(Clone, Copy)]
enum InlineTokenKind {
    Glyph { glyph_idx: u32 },
    Image(u32),
    Break,
    FloatAnchor { box_idx: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakKind {
    None,
    Soft,
    Hard,
}

/// Mid-word wrapping allowed before this token, from `word-break` /
/// `overflow-wrap`. Unlike a `Soft` break the token is kept, not discarded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenWrap {
    /// Break only at soft-break opportunities.
    Normal,
    /// `overflow-wrap: break-word`/`anywhere`: break mid-word only when the
    /// word cannot fit on a line by itself (no soft break on the line).
    BreakWord,
    /// `word-break: break-all`: break between any two characters, filling
    /// each line completely.
    Anywhere,
}

struct BrokenLine {
    start: usize,
    end: usize,
    indent: f64,
    tab_origin: f64,
    align: TextAlign,
    is_last_line: bool,
}

struct TokenPlacement {
    x: f64,
    advance: f64,
    y_offset: f32,
}

struct LineLayout {
    glyph_range: Range<u32>,
    width: f64,
    x_offset: f64,
    line_height: f64,
    baseline: f64,
    word_spacing: f64,
    letter_spacing: f64,
    placements: Vec<TokenPlacement>,
}

struct TokenSpanSummary {
    glyph_range: Range<u32>,
    has_text: bool,
    has_images: bool,
    width: f64,
}

struct LineBreakContext<'a> {
    tokens: &'a [InlineToken],
    available_width: f64,
    text_indent: f64,
    text_indent_hanging: bool,
    position: Point,
}

struct LineBreakState {
    x: f64,
    line_start: usize,
    last_break: Option<usize>,
    is_first_line: bool,
    line_indent: f64,
    tab_origin: f64,
    line_width_limit: f64,
}

impl<'a, 'out> crate::layout::LayoutContext<'a, 'out> {
    /// Lays out one inline run range from start to finish.
    ///
    /// This is the entry point used by box layout. It tokenizes the runs,
    /// breaks them into logical lines, and then emits fragments from those
    /// lines in a separate pass.
    pub(crate) fn layout_runs(
        &mut self, run_range: Range<u32>, position: Point, available_width: f64, text_indent: f64, text_indent_hanging: bool, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign, float_top_align: bool,
    ) -> Size {
        let timing_started = Instant::now();
        if run_range.start >= run_range.end {
            self.record_timing(|t| t.layout_runs += timing_started.elapsed());
            return Size::ZERO;
        }
        let float_affected =
            self.inline_runs()[run_range.start as usize..run_range.end as usize].iter().any(|run| matches!(run.kind, InlineKind::FloatAnchor { .. })) || self.current_float_context().map(|ctx| !ctx.is_empty()).unwrap_or(false);
        if float_affected {
            let size = self.layout_runs_with_floats(run_range, position, available_width, text_indent, text_indent_hanging, text_indent_each_line, text_align, text_align_last, float_top_align);
            self.record_timing(|t| t.layout_runs += timing_started.elapsed());
            return size;
        }

        let tokens = self.build_inline_tokens(run_range, (available_width - text_indent.max(0.0)).max(0.0));
        if tokens.is_empty() {
            self.record_timing(|t| t.layout_runs += timing_started.elapsed());
            return Size::ZERO;
        }
        let ctx = LineBreakContext { tokens: &tokens, available_width, text_indent, text_indent_hanging, position };
        let use_knuth = knuth_enabled() && !tokens.iter().any(|token| token.tab_advance.is_some()) && (matches!(text_align, TextAlign::Justify) || self.force_justify);
        let lines = if use_knuth { self.break_lines_knuth(&ctx, text_indent_each_line, text_align, text_align_last) } else { self.break_lines(&ctx, text_indent_each_line, text_align, text_align_last) };
        let size = self.emit_lines(&ctx, &lines, float_top_align);
        self.record_timing(|t| t.layout_runs += timing_started.elapsed());
        size
    }

    fn layout_runs_with_floats(
        &mut self, run_range: Range<u32>, position: Point, available_width: f64, text_indent: f64, text_indent_hanging: bool, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign, float_top_align: bool,
    ) -> Size {
        let timing_started = Instant::now();
        let mut floated_boxes = Vec::new();
        for run in &self.inline_runs()[run_range.start as usize..run_range.end as usize] {
            if let InlineKind::FloatAnchor { box_idx } = run.kind {
                let box_idx = box_idx as usize;
                if !floated_boxes.contains(&box_idx) {
                    floated_boxes.push(box_idx);
                }
            }
        }
        for &box_idx in &floated_boxes {
            self.place_float_anchor(box_idx, position, available_width);
        }

        let tokens = if floated_boxes.is_empty() {
            self.build_inline_tokens(run_range, (available_width - text_indent.max(0.0)).max(0.0))
        } else {
            let filtered_runs: Vec<InlineRun> =
                self.inline_runs()[run_range.start as usize..run_range.end as usize].iter().filter(|run| !matches!(run.kind, InlineKind::FloatAnchor { .. }) && !floated_boxes.contains(&(run.box_idx as usize))).cloned().collect();
            self.build_inline_tokens_from_runs(&filtered_runs, (available_width - text_indent.max(0.0)).max(0.0))
        };
        if tokens.is_empty() {
            self.record_timing(|t| t.layout_runs_with_floats += timing_started.elapsed());
            return Size::ZERO;
        }

        let mut y = 0.0;
        let mut max_width = 0.0f64;
        let mut start = 0usize;
        let mut is_first_line = true;
        while start < tokens.len() {
            let (left, right) = self.current_float_context().map(|ctx| ctx.available(position.y + y, self.line_height.max(1.0), position.x, position.x + available_width)).unwrap_or((position.x, position.x + available_width));
            let line_indent = indent_for_line(text_indent, text_indent_hanging, is_first_line);
            let line_width_limit = (right - left - line_indent).max(0.0);
            let tab_origin = (left - position.x) + line_indent;
            let (line, next_start) = self.break_one_line(&tokens, start, line_width_limit, line_indent, tab_origin, is_first_line, text_indent_each_line, text_align, text_align_last);
            let span = &tokens[line.start..line.end];
            let layout = self.measure_line(span, line.tab_origin, line_width_limit, line.is_last_line, line.align, float_top_align);
            max_width = max_width.max(layout.width + line.indent);
            let line_origin = Point::new(left, position.y + y);
            y += self.write_line_fragments(span, &line, 0.0, line_origin, &layout);
            start = next_start.max(line.end);
            if start <= line.start && start < tokens.len() {
                start = line.end.saturating_add(1);
            }
            is_first_line = false;
        }

        let float_bottom = self.current_float_context().map(|ctx| ctx.max_bottom() - position.y).unwrap_or(0.0).max(0.0);
        let size = Size::new(max_width, y.max(float_bottom));
        self.record_timing(|t| t.layout_runs_with_floats += timing_started.elapsed());
        size
    }

    fn place_float_anchor(&mut self, box_idx: usize, position: Point, available_width: f64) {
        let timing_started = Instant::now();
        let side = self.box_float(box_idx);
        let style = self.style(box_idx);
        let margin_left = style.margin_left().resolve(available_width);
        let margin_top = style.margin_top().resolve(available_width);
        let margin_right = style.margin_right().resolve(available_width);
        let margin_bottom = style.margin_bottom().resolve(available_width);
        let float_point = Point::new(position.x + margin_left, position.y + margin_top);
        self.set_box_point(box_idx, float_point);
        let size = self.layout_box(box_idx, available_width, None, 0.0);
        // Lines are excluded from the float's *margin* box (CSS §9.5), not its
        // border box — drop caps rely on a negative margin-bottom to release
        // the last wrapped line early.
        let band = crate::layout::FloatBand {
            left: position.x,
            right: float_point.x + size.width + margin_right,
            top: position.y,
            bottom: float_point.y + size.height + margin_bottom,
            side: match side {
                Float::Right => FloatSide::Right,
                _ => FloatSide::Left,
            },
        };
        if let Some(ctx) = self.current_float_context_mut() {
            ctx.add_band(band);
        }
        self.record_timing(|t| t.place_float_anchor += timing_started.elapsed());
    }

    fn break_one_line(
        &self, tokens: &[InlineToken], start: usize, line_width_limit: f64, line_indent: f64, tab_origin: f64, is_first_line: bool, _text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign,
    ) -> (BrokenLine, usize) {
        let mut state = LineBreakState { x: 0.0, line_start: start, last_break: None, is_first_line, line_indent, tab_origin, line_width_limit };

        let mut i = start;
        while i < tokens.len() {
            let token = &tokens[i];
            if let InlineTokenKind::FloatAnchor { box_idx } = token.kind {
                let _ = box_idx;
                i += 1;
                continue;
            }

            if token.break_kind == BreakKind::Hard {
                let line = self.make_broken_line(tokens, &state, i, text_align_last, true, true);
                let next_start = self.trim_leading_soft_tokens(tokens, i + 1);
                return (line, next_start);
            }

            let advance = token.advance_at(tab_origin + state.x);
            if state.x + advance > state.line_width_limit && i > state.line_start && !(state.last_break.is_none() && self.keep_with_previous_token_for_tokens(tokens, i)) {
                let break_mid_word = match token.wrap {
                    TokenWrap::Anywhere => true,
                    TokenWrap::BreakWord => state.last_break.is_none(),
                    TokenWrap::Normal => false,
                };
                if break_mid_word {
                    let line = self.make_broken_line(tokens, &state, i, text_align, false, false);
                    let next_start = self.trim_leading_soft_tokens(tokens, i);
                    return (line, next_start);
                }
                if let Some(break_at) = state.last_break.filter(|&bp| bp > state.line_start) {
                    let line = self.make_broken_line(tokens, &state, break_at, text_align, false, false);
                    let next_start = self.trim_leading_soft_tokens(tokens, break_at);
                    return (line, next_start);
                }
            }

            if token.break_kind == BreakKind::Soft {
                state.last_break = Some(i);
            }
            state.x += advance;
            i += 1;
        }

        let line = self.make_broken_line(tokens, &state, tokens.len(), text_align_last, true, false);
        (line, tokens.len())
    }

    fn make_broken_line(&self, tokens: &[InlineToken], state: &LineBreakState, end: usize, align: TextAlign, is_last_line: bool, preserve_empty: bool) -> BrokenLine {
        let trimmed_end = self.trim_trailing_line_whitespace_tokens(tokens, state.line_start, end);
        if trimmed_end > state.line_start || preserve_empty {
            BrokenLine { start: state.line_start, end: trimmed_end, indent: state.line_indent, tab_origin: state.tab_origin, align, is_last_line }
        } else {
            BrokenLine { start: state.line_start, end: state.line_start, indent: state.line_indent, tab_origin: state.tab_origin, align, is_last_line }
        }
    }

    fn keep_with_previous_token_for_tokens(&self, tokens: &[InlineToken], i: usize) -> bool {
        if i == 0 {
            return false;
        }
        let token = &tokens[i];
        if !matches!(token.vertical_align, VerticalAlignValue::Super | VerticalAlignValue::Sub) {
            return false;
        }
        let previous = &tokens[i - 1];
        previous.break_kind == BreakKind::None
    }

    fn trim_leading_soft_tokens(&self, tokens: &[InlineToken], mut start: usize) -> usize {
        while start < tokens.len() && self.should_trim_leading_soft_token(&tokens[start]) {
            start += 1;
        }
        start
    }

    fn trim_trailing_line_whitespace_tokens(&self, tokens: &[InlineToken], start: usize, end: usize) -> usize {
        let mut trimmed_end = end;
        while trimmed_end > start && self.should_trim_leading_soft_token(&tokens[trimmed_end - 1]) {
            trimmed_end -= 1;
        }
        trimmed_end
    }

    /// Breaks a token stream into logical lines without mutating the document.
    ///
    /// This keeps the line-breaking state machine separate from fragment emission
    /// and returns the spans plus per-line metadata needed by the emit phase.
    fn break_lines(&self, ctx: &LineBreakContext<'_>, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign) -> Vec<BrokenLine> {
        let timing_started = Instant::now();
        let mut state = LineBreakState {
            x: 0.0,
            line_start: 0,
            last_break: None,
            is_first_line: true,
            line_indent: indent_for_line(ctx.text_indent, ctx.text_indent_hanging, true),
            tab_origin: indent_for_line(ctx.text_indent, ctx.text_indent_hanging, true),
            line_width_limit: (ctx.available_width - indent_for_line(ctx.text_indent, ctx.text_indent_hanging, true)).max(0.0),
        };
        let mut lines = Vec::new();
        let mut i = 0usize;
        while i < ctx.tokens.len() {
            let token = &ctx.tokens[i];

            // Hard break (<br> or a preserved newline): end the line here even if
            // empty, and resume on the token after the break.
            if token.break_kind == BreakKind::Hard {
                self.push_broken_line(&mut lines, ctx, &state, i, text_align_last, true, true);
                self.reset_line_break_state(ctx, &mut state, i + 1, text_indent_each_line, false);
                i = state.line_start;
                continue;
            }

            // The token would overflow the line. Wrap at the last soft break if
            // there is one; with `word-break: break-all` or an emergency
            // `overflow-wrap` break, wrap right before the overflowing token
            // instead (the token is kept, unlike a discarded soft break).
            // Otherwise fall through and keep consuming so a too-long word
            // overflows rather than breaking mid-word. A super/sub marker is
            // kept attached to the preceding token instead of being stranded.
            let advance = token.advance_at(state.tab_origin + state.x);
            if state.x + advance > state.line_width_limit && i > state.line_start && !(state.last_break.is_none() && self.keep_with_previous_token(ctx, i)) {
                let break_mid_word = match token.wrap {
                    TokenWrap::Anywhere => true,
                    TokenWrap::BreakWord => state.last_break.is_none(),
                    TokenWrap::Normal => false,
                };
                if break_mid_word {
                    self.push_broken_line(&mut lines, ctx, &state, i, text_align, false, false);
                    self.reset_line_break_state(ctx, &mut state, i, false, true);
                    i = state.line_start;
                    continue;
                }
                if let Some(break_at) = state.last_break.filter(|&bp| bp > state.line_start) {
                    self.push_broken_line(&mut lines, ctx, &state, break_at, text_align, false, false);
                    self.reset_line_break_state(ctx, &mut state, break_at, false, true);
                    i = state.line_start;
                    continue;
                }
            }

            // Advance: record soft-break opportunities and consume the token.
            if token.break_kind == BreakKind::Soft {
                state.last_break = Some(i);
            }
            state.x += advance;
            i += 1;
        }

        if state.line_start < ctx.tokens.len() {
            self.push_broken_line(&mut lines, ctx, &state, ctx.tokens.len(), text_align_last, true, false);
        }
        self.record_timing(|t| t.break_lines += timing_started.elapsed());
        lines
    }

    /// Keeps footnote-like superscript or subscript markers attached to the preceding token.
    ///
    /// Without this, a narrow line can wrap immediately before the marker and
    /// leave it stranded on the next line even though it semantically belongs
    /// to the preceding word or punctuation.
    fn keep_with_previous_token(&self, ctx: &LineBreakContext<'_>, i: usize) -> bool {
        if i == 0 {
            return false;
        }

        let token = &ctx.tokens[i];
        if !matches!(token.vertical_align, VerticalAlignValue::Super | VerticalAlignValue::Sub) {
            return false;
        }

        let previous = &ctx.tokens[i - 1];
        previous.break_kind == BreakKind::None
    }

    /// Appends one broken line record using the current line-break state.
    ///
    /// The resulting record is later consumed by `emit_lines`.
    fn push_broken_line(&self, lines: &mut Vec<BrokenLine>, ctx: &LineBreakContext<'_>, state: &LineBreakState, end: usize, align: TextAlign, is_last_line: bool, preserve_empty: bool) {
        let trimmed_end = self.trim_trailing_line_whitespace(ctx, state.line_start, end);
        if trimmed_end > state.line_start || preserve_empty {
            lines.push(BrokenLine { start: state.line_start, end: trimmed_end, indent: state.line_indent, tab_origin: state.tab_origin, align, is_last_line });
        }
    }

    /// Resets the line-break state after a line has been committed.
    ///
    /// Both hard-break and soft-wrap transitions use this shared post-line reset.
    fn reset_line_break_state(&self, ctx: &LineBreakContext<'_>, state: &mut LineBreakState, mut next_start: usize, is_first_line: bool, trim_breakables: bool) {
        if trim_breakables {
            while next_start < ctx.tokens.len() && self.should_trim_leading_soft_token(&ctx.tokens[next_start]) {
                next_start += 1;
            }
        }
        state.x = 0.0;
        state.line_start = next_start;
        state.last_break = None;
        state.is_first_line = is_first_line;
        state.line_indent = indent_for_line(ctx.text_indent, ctx.text_indent_hanging, is_first_line);
        state.tab_origin = state.line_indent;
        state.line_width_limit = (ctx.available_width - state.line_indent).max(0.0);
    }

    /// Measures and emits broken lines, returning the final laid-out size.
    ///
    /// This is the write-side pass of the pipeline. It turns `BrokenLine`
    /// records into measured line layouts and then writes document fragments.
    fn emit_lines(&mut self, ctx: &LineBreakContext<'_>, lines: &[BrokenLine], float_top_align: bool) -> Size {
        let timing_started = Instant::now();
        let mut y = 0.0;
        let mut max_width = 0.0f64;
        for line in lines {
            let span = &ctx.tokens[line.start..line.end];
            let layout = self.measure_line(span, line.tab_origin, ctx.available_width - line.indent, line.is_last_line, line.align, float_top_align);
            max_width = max_width.max(layout.width + line.indent);
            y += self.write_line_fragments(span, line, y, ctx.position, &layout);
        }
        let size = Size::new(max_width, y);
        self.record_timing(|t| t.emit_lines += timing_started.elapsed());
        size
    }

    fn create_text_token(&self, glyph_idx: u32, style: crate::document::StyleView, tab_reference_style: crate::document::StyleView, white_space: WhiteSpace, vertical_align: VerticalAlignValue) -> InlineToken {
        let glyph_index = self.glyph_at(glyph_idx as usize).unwrap_or_default();
        let metric = self.glyph_metric(glyph_index);
        let c = metric.ch();

        let width = metric.advance() as f64 + style.letter_spacing() as f64 + if c == ' ' { style.word_spacing() as f64 } else { 0.0 };
        let tab_advance = (c == '\t' && white_space.preserves_spaces()).then(|| {
            let ch_advance = tab_reference_style.font_size() as f64 * 0.5;
            let interval = match style.tab_size().kind() {
                TabSizeKind::Spaces => style.tab_size().value() as f64 * (ch_advance + tab_reference_style.letter_spacing() as f64 + tab_reference_style.word_spacing() as f64).max(0.0),
                TabSizeKind::LengthPx => style.tab_size().value() as f64,
            };
            TabAdvance { interval: interval.min(f32::MAX as f64), min_advance: (ch_advance * 0.5).min(f32::MAX as f64) }
        });
        let (ascent, descent) = (metric.ascent() as f64, metric.descent() as f64);
        let break_kind = Self::break_kind(c, white_space);
        let font_size = style.font_size();
        let line_height = if style.line_height() > 0.0 { style.line_height() as f64 } else { style.font_size() as f64 * 1.2 };
        let wrap = Self::token_wrap(style, white_space);
        InlineToken { kind: InlineTokenKind::Glyph { glyph_idx }, width, tab_advance, ascent, descent, line_height, font_size, vertical_align, break_kind, white_space, wrap }
    }

    /// Mid-word wrap mode for a text token. `white-space` values that forbid
    /// wrapping suppress mid-word breaks entirely.
    fn token_wrap(style: crate::document::StyleView, white_space: WhiteSpace) -> TokenWrap {
        if !white_space.allows_wrap() {
            return TokenWrap::Normal;
        }
        if style.word_break() == WordBreak::BreakAll {
            return TokenWrap::Anywhere;
        }
        match style.overflow_wrap() {
            OverflowWrap::BreakWord | OverflowWrap::Anywhere => TokenWrap::BreakWord,
            OverflowWrap::Normal => TokenWrap::Normal,
        }
    }

    fn create_image_token(&self, image_idx: &u32, style: crate::document::StyleView, vertical_align: VerticalAlignValue, max_width: f64) -> InlineToken {
        let (mut width, mut height) = self.image_display_size(*image_idx);
        // Clamp to the available width (the `img { max-width: 100% }` reset every
        // EPUB assumes): shrink an oversized image to fit, but never enlarge a
        // small one to fill the column.
        if max_width > 0.0 && width > max_width {
            let scale = max_width / width;
            width = max_width;
            height = (height * scale).max(1.0);
        }
        let font_size = style.font_size();
        let line_height = if style.line_height() > 0.0 { style.line_height() as f64 } else { style.font_size() as f64 * 1.2 };
        InlineToken {
            kind: InlineTokenKind::Image(*image_idx),
            width,
            tab_advance: None,
            ascent: height,
            descent: 0.0,
            line_height,
            font_size,
            vertical_align,
            break_kind: BreakKind::None,
            white_space: style.white_space(),
            wrap: TokenWrap::Normal,
        }
    }

    fn create_break_token(&self, style: crate::document::StyleView, vertical_align: VerticalAlignValue) -> InlineToken {
        let line_height = if style.line_height() > 0.0 { style.line_height() as f64 } else { style.font_size() as f64 * 1.2 };
        InlineToken {
            kind: InlineTokenKind::Break,
            width: 0.0,
            tab_advance: None,
            ascent: 0.0,
            descent: 0.0,
            line_height,
            font_size: style.font_size(),
            vertical_align,
            break_kind: BreakKind::Hard,
            white_space: style.white_space(),
            wrap: TokenWrap::Normal,
        }
    }

    fn create_float_anchor_token(&self, _box_idx: u32, style: crate::document::StyleView, vertical_align: VerticalAlignValue) -> InlineToken {
        let line_height = if style.line_height() > 0.0 { style.line_height() as f64 } else { style.font_size() as f64 * 1.2 };
        InlineToken {
            kind: InlineTokenKind::FloatAnchor { box_idx: _box_idx },
            width: 0.0,
            tab_advance: None,
            ascent: 0.0,
            descent: 0.0,
            line_height,
            font_size: style.font_size(),
            vertical_align,
            break_kind: BreakKind::None,
            white_space: style.white_space(),
            wrap: TokenWrap::Normal,
        }
    }

    /// Converts inline runs into layout tokens.
    ///
    /// Each token carries the geometry and break metadata needed by the line
    /// breaker plus the placement inputs needed later by line measurement.

    fn build_inline_tokens(&self, run_range: Range<u32>, max_width: f64) -> Vec<InlineToken> {
        let timing_started = Instant::now();
        let tokens = self.build_inline_tokens_from_runs(&self.inline_runs()[run_range.start as usize..run_range.end as usize], max_width);
        // Nested timing is measured in build_inline_tokens_from_runs; this captures the outer wrapper.
        self.record_timing(|t| t.build_inline_tokens += timing_started.elapsed());
        tokens
    }

    fn build_inline_tokens_from_runs(&self, runs: &[InlineRun], max_width: f64) -> Vec<InlineToken> {
        let timing_started = Instant::now();
        let mut tokens = Vec::new();
        for run in runs {
            let style = self.style(run.box_idx as usize);
            let tab_reference_style = self.tab_reference_style(run.box_idx as usize);
            let vertical_align = effective_vertical_align(self, run.box_idx as usize);
            let white_space = style.white_space();
            match &run.kind {
                InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } => tokens.extend(glyphs.clone().map(|i| self.create_text_token(i, style, tab_reference_style, white_space, vertical_align))),
                InlineKind::Image { image_idx } => {
                    tokens.push(self.create_image_token(image_idx, style, vertical_align, max_width));
                }
                InlineKind::Break => tokens.push(self.create_break_token(style, vertical_align)),
                InlineKind::FloatAnchor { box_idx } => tokens.push(self.create_float_anchor_token(*box_idx, style, vertical_align)),
            }
        }
        self.record_timing(|t| t.build_inline_tokens_from_runs += timing_started.elapsed());
        tokens
    }

    /// Style supplying the space/ch metric used by number-valued `tab-size`.
    /// CSS defines it on the nearest block container rather than the inline
    /// span that happens to contain the tab.
    pub(super) fn tab_reference_style(&self, box_idx: usize) -> crate::document::StyleView<'_> {
        let mut current = Some(box_idx);
        while let Some(idx) = current {
            let style = self.style(idx);
            if style.display() != crate::document::Display::Inline {
                return style;
            }
            current = self.get_parent(idx);
        }
        self.style(box_idx)
    }

    /// Computes line metrics directly from token ascent and descent values.
    ///
    /// This is the fallback path for mixed-content lines where glyph-based text
    /// metrics are not used.
    fn line_metrics_from_tokens(&self, span: &[InlineToken]) -> (f64, f64) {
        // Text contributes its leaded extents (CSS §10.8.1): half the leading —
        // line-height minus glyph extents, possibly negative as in a drop cap
        // with line-height < 1 — above the ascent and half below the descent.
        // Replaced/synthetic tokens contribute their raw box and line-height.
        let (max_ascent, max_descent, max_line_height) = span.iter().fold((0.0f64, 0.0f64, 0.0f64), |(max_ascent, max_descent, max_line_height), token| match token.kind {
            InlineTokenKind::Glyph { .. } => {
                let half_leading = (token.line_height - (token.ascent + token.descent)) / 2.0;
                (max_ascent.max(token.ascent + half_leading), max_descent.max(token.descent + half_leading), max_line_height)
            }
            _ => (max_ascent.max(token.ascent), max_descent.max(token.descent), max_line_height.max(token.line_height)),
        });
        compute_baseline(max_ascent, max_descent, max_line_height.max(self.line_height))
    }

    /// Converts `vertical-align` into a baseline-relative y offset.
    ///
    /// Both line measurement and fragment writing call this so they agree on the
    /// final vertical placement of each token.
    fn vertical_align_offset(&self, token: &InlineToken, line_height: f64, baseline: f64) -> f64 {
        match token.vertical_align {
            VerticalAlignValue::Baseline => 0.0,
            VerticalAlignValue::Super => token.font_size as f64 * 0.5,
            VerticalAlignValue::Sub => -(token.font_size as f64 * 0.25),
            VerticalAlignValue::Length(px) => px as f64,
            VerticalAlignValue::Percent(pct) => line_height * pct as f64,
            VerticalAlignValue::Top | VerticalAlignValue::TextTop => baseline - token.ascent,
            VerticalAlignValue::Bottom | VerticalAlignValue::TextBottom => baseline + token.descent - line_height,
            VerticalAlignValue::Middle => baseline - token.ascent + (token.ascent + token.descent - line_height) / 2.0,
        }
    }

    /// Measures a token span as a fully placed line without mutating the document.
    ///
    /// This computes glyph span, line metrics, justification, and per-token
    /// placements so the later write phase can emit fragments without
    /// reinterpreting the token span.
    fn measure_line(&self, span: &[InlineToken], tab_origin: f64, line_width_limit: f64, is_last_line: bool, text_align: TextAlign, float_top_align: bool) -> LineLayout {
        let timing_started = Instant::now();
        let summary = self.summarize_token_span(span, tab_origin);
        let (line_height, baseline, word_spacing, letter_spacing) = self.base_line_metrics(span, &summary, line_width_limit, is_last_line, text_align, float_top_align);
        let (line_height, baseline) = self.adjusted_line_metrics(span, line_height, baseline, float_top_align);
        let x_offset = self.line_x_offset(line_width_limit, summary.width, text_align);
        let placements = self.build_token_placements(span, tab_origin, line_height, baseline);

        let layout = LineLayout { glyph_range: summary.glyph_range, width: summary.width, x_offset, line_height, baseline, word_spacing, letter_spacing, placements };
        self.record_timing(|t| t.measure_line += timing_started.elapsed());
        layout
    }

    /// Computes the horizontal start offset for non-justified aligned lines.
    ///
    /// Inline placement uses this to support `left`, `center`, and `right`
    /// alignment instead of always starting each line at x=0.
    fn line_x_offset(&self, line_width_limit: f64, line_width: f64, text_align: TextAlign) -> f64 {
        let remaining = (line_width_limit - line_width).max(0.0);
        match text_align {
            TextAlign::Center => remaining / 2.0,
            TextAlign::Right => remaining,
            _ => 0.0,
        }
    }

    /// Summarizes the content mix and glyph coverage of a token span.
    fn summarize_token_span(&self, span: &[InlineToken], tab_origin: f64) -> TokenSpanSummary {
        let (glyph_start, glyph_end, has_text, has_images, width) = span.iter().fold((u32::MAX, 0u32, false, false, 0.0f64), |(glyph_start, glyph_end, has_text, has_images, width), token| {
            (
                match token.kind {
                    InlineTokenKind::Glyph { glyph_idx } => glyph_start.min(glyph_idx),
                    InlineTokenKind::Image(_) | InlineTokenKind::Break | InlineTokenKind::FloatAnchor { .. } => glyph_start,
                },
                match token.kind {
                    InlineTokenKind::Glyph { glyph_idx } => glyph_end.max(glyph_idx + 1),
                    InlineTokenKind::Image(_) | InlineTokenKind::Break | InlineTokenKind::FloatAnchor { .. } => glyph_end,
                },
                has_text || matches!(token.kind, InlineTokenKind::Glyph { .. }),
                has_images || matches!(token.kind, InlineTokenKind::Image(_)),
                width + token.advance_at(tab_origin + width),
            )
        });
        TokenSpanSummary { glyph_range: if has_text { glyph_start..glyph_end } else { 0..0 }, has_text, has_images, width }
    }

    /// Chooses the initial line metrics before vertical-align adjustments are applied.
    fn base_line_metrics(&self, span: &[InlineToken], summary: &TokenSpanSummary, line_width_limit: f64, is_last_line: bool, text_align: TextAlign, float_top_align: bool) -> (f64, f64, f64, f64) {
        let (line_height, baseline) = self.line_metrics_from_tokens(span);
        if summary.has_text && !summary.has_images {
            let needs_justification = !is_last_line && (text_align == TextAlign::Justify || (self.force_justify && text_align == TextAlign::Left));
            let disable_justify = needs_justification && span.iter().any(|token| token.white_space.preserves_newlines());
            let (word_spacing, letter_spacing) =
                compute_justification(self, self.glyph_metrics(), summary.glyph_range.start, summary.glyph_range.end, summary.width, line_width_limit, is_last_line, text_align, disable_justify, self.force_justify);
            if float_top_align {
                let baseline_offset = (summary.glyph_range.start..summary.glyph_range.end).map(|i| self.glyph_metric(self.glyph_at(i as usize).unwrap_or_default()).baseline_offset() as f64).fold(0.0f64, f64::max);
                (line_height, baseline_offset, word_spacing, letter_spacing)
            } else {
                (line_height, baseline, word_spacing, letter_spacing)
            }
        } else {
            (line_height, baseline, 0.0, 0.0)
        }
    }

    /// Recomputes the line box after per-token vertical alignment offsets are applied.
    fn adjusted_line_metrics(&self, span: &[InlineToken], line_height: f64, baseline: f64, float_top_align: bool) -> (f64, f64) {
        if float_top_align {
            return (line_height, baseline);
        }
        let (adjusted_ascent, adjusted_descent) = span
            .iter()
            .map(|token| {
                let offset = self.vertical_align_offset(token, line_height, baseline);
                ((token.ascent + offset).max(0.0), (token.descent - offset).max(0.0))
            })
            .fold((0.0f64, 0.0f64), |(max_ascent, max_descent), (ascent, descent)| (max_ascent.max(ascent), max_descent.max(descent)));
        compute_baseline(adjusted_ascent, adjusted_descent, line_height)
    }

    /// Builds final per-token placements for a measured line box.
    fn build_token_placements(&self, span: &[InlineToken], tab_origin: f64, line_height: f64, baseline: f64) -> Vec<TokenPlacement> {
        let mut curr_x = 0.0;
        span.iter()
            .map(|token| {
                let advance = token.advance_at(tab_origin + curr_x);
                let placement = TokenPlacement { x: curr_x, advance, y_offset: self.vertical_align_offset(token, line_height, baseline) as f32 };
                curr_x += advance;
                placement
            })
            .collect()
    }

    /// Writes fragments for one broken line using a precomputed line layout.
    ///
    /// The write phase only mutates the document; all layout decisions have
    /// already been made by `break_lines` and `measure_line`.
    fn write_line_fragments(&mut self, span: &[InlineToken], broken_line: &BrokenLine, y: f64, position: Point, line: &LineLayout) -> f64 {
        let timing_started = Instant::now();
        let point = Point::new(position.x + broken_line.indent + line.x_offset, position.y + y);
        let layout = self.layout_state_mut();
        let line_idx = layout.lines.len();
        layout.lines.push(Line { glyphs: line.glyph_range.clone(), point, height: line.line_height, baseline: line.baseline, word_spacing: line.word_spacing, letter_spacing: line.letter_spacing });
        layout.line_glyph_offsets.push(Vec::new());
        layout.line_glyph_advances.push(Vec::new());

        for (token, placement) in span.iter().zip(&line.placements) {
            let offset = placement.y_offset as f64;
            if let InlineTokenKind::Image(image_idx) = token.kind {
                layout.image_fragments.push(ImageFragment { line_idx, image_idx, offset: Point::new(placement.x, line.baseline - token.ascent - offset), size: Size::new(placement.advance, token.ascent) });
            } else if let InlineTokenKind::Glyph { glyph_idx } = token.kind {
                Self::push_glyph_offset_run(&mut layout.line_glyph_offsets[line_idx], glyph_idx, offset as f32);
                if token.tab_advance.is_some() {
                    layout.line_glyph_advances[line_idx].push(GlyphAdvanceRun { range: glyph_idx..glyph_idx + 1, advance: placement.advance as f32 });
                }
            }
        }
        self.record_timing(|t| t.write_line_fragments += timing_started.elapsed());
        line.line_height
    }

    /// Appends or extends a glyph offset run.
    ///
    /// Adjacent glyphs with the same vertical offset are merged so the document
    /// stores compact offset ranges instead of one entry per glyph.
    fn push_glyph_offset_run(offsets: &mut Vec<GlyphOffsetRun>, glyph_idx: u32, offset: f32) {
        if let Some(last) = offsets.last_mut().filter(|last| last.range.end == glyph_idx && last.offset == offset) {
            last.range.end = glyph_idx + 1;
        } else {
            offsets.push(GlyphOffsetRun { range: glyph_idx..glyph_idx + 1, offset });
        }
    }

    /// Classifies a glyph as a hard break, soft break, or non-break.
    ///
    /// This replaces the old pair of `breakable` and `hard_break` booleans.
    fn break_kind(character: char, white_space: WhiteSpace) -> BreakKind {
        if character == '\n' && white_space.preserves_newlines() {
            return BreakKind::Hard;
        }
        if character == '\u{00A0}' || !white_space.allows_wrap() {
            return BreakKind::None;
        }
        match character {
            ' ' | '\t' | '\n' => BreakKind::Soft,
            _ => BreakKind::None,
        }
    }

    fn should_trim_leading_soft_token(&self, token: &InlineToken) -> bool {
        token.break_kind == BreakKind::Soft && !token.white_space.preserves_spaces()
    }

    fn trim_trailing_line_whitespace(&self, ctx: &LineBreakContext<'_>, start: usize, end: usize) -> usize {
        let mut trimmed_end = end;
        while trimmed_end > start && self.should_trim_leading_soft_token(&ctx.tokens[trimmed_end - 1]) {
            trimmed_end -= 1;
        }
        trimmed_end
    }
}

impl<'a, 'out> crate::layout::LayoutContext<'a, 'out> {
    /// Knuth–Plass optimal line breaking — a drop-in alternative to
    /// `break_lines` with the same signature and return type.
    ///
    /// Splits the token stream into paragraphs at hard breaks, optimises each
    /// one with the standalone solver, and maps the chosen breakpoints back
    /// onto `BrokenLine` records using the same trailing-whitespace trimming and
    /// indent rules as the greedy breaker, so the emit phase is unchanged.
    /// Falls back to `break_lines` whenever the solver reports no feasible set
    /// of breaks (e.g. a token wider than the column), so it never renders worse.
    fn break_lines_knuth(&self, ctx: &LineBreakContext<'_>, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign) -> Vec<BrokenLine> {
        let timing_started = Instant::now();
        use super::knuth_plass as kp;
        let tokens = ctx.tokens;

        // Paragraph segments delimited by hard breaks (<br>, preserved newlines).
        let mut segments: Vec<(usize, usize)> = Vec::new();
        let mut seg_start = 0usize;
        for (i, token) in tokens.iter().enumerate() {
            if token.break_kind == BreakKind::Hard {
                segments.push((seg_start, i));
                seg_start = i + 1;
            }
        }
        segments.push((seg_start, tokens.len()));
        let seg_count = segments.len();

        let mut lines: Vec<BrokenLine> = Vec::new();
        let mut global_line = 0usize;

        for (seg_idx, (start, end)) in segments.into_iter().enumerate() {
            if start >= end {
                // An empty segment is a blank line from consecutive hard breaks.
                // A trailing empty segment adds no line, matching the greedy
                // breaker which never emits a line past the final content.
                if seg_idx + 1 == seg_count {
                    continue;
                }
                let indent = indent_for_line(ctx.text_indent, ctx.text_indent_hanging, global_line == 0 || text_indent_each_line);
                lines.push(BrokenLine { start, end, indent, tab_origin: indent, align: text_align_last, is_last_line: true });
                global_line += 1;
                continue;
            }

            let seg_base_line = global_line;
            let items = Self::build_kp_items(&tokens[start..end]);
            let line_width = |line: usize| (ctx.available_width - indent_for_line(ctx.text_indent, ctx.text_indent_hanging, seg_base_line + line == 0 || text_indent_each_line)).max(0.0);

            let Some(breaks) = kp::break_lines(&items, line_width, KP_TOLERANCE) else {
                // Infeasible paragraph: abandon KP for the whole run and use the
                // greedy result so the output is always renderable.
                return self.break_lines(ctx, text_indent_each_line, text_align, text_align_last);
            };

            let content_len = end - start;
            let break_count = breaks.len();
            let mut line_start = start;
            for (k, item_idx) in breaks.into_iter().enumerate() {
                let is_seg_last = k + 1 == break_count;
                // Items map 1:1 onto tokens for [0, content_len); indices beyond
                // that are the appended finishing glue/penalty marking the end.
                let break_token = if item_idx < content_len { start + item_idx } else { end };
                let line_end = self.trim_trailing_line_whitespace(ctx, line_start, break_token);
                let indent = indent_for_line(ctx.text_indent, ctx.text_indent_hanging, global_line == 0 || text_indent_each_line);
                let (align, is_last_line) = if is_seg_last { (text_align_last, true) } else { (text_align, false) };
                if line_end > line_start {
                    lines.push(BrokenLine { start: line_start, end: line_end, indent, tab_origin: indent, align, is_last_line });
                    global_line += 1;
                }
                // The break token is the discarded inter-word glue; resume after it.
                line_start = if item_idx < content_len { break_token + 1 } else { end };
            }
        }

        self.record_timing(|t| t.break_lines_knuth += timing_started.elapsed());
        lines
    }

    /// Converts inline tokens into Knuth–Plass items (1:1), then appends the
    /// finishing glue and forced break that terminate the paragraph.
    fn build_kp_items(tokens: &[InlineToken]) -> Vec<super::knuth_plass::Item> {
        use super::knuth_plass::Item;
        let mut items = Vec::with_capacity(tokens.len() + 2);
        for token in tokens {
            match token.break_kind {
                // A soft break is an inter-word space: breakable glue whose
                // stretch/shrink approximate TeX interword spacing.
                BreakKind::Soft => items.push(Item::glue(token.width, token.width * 0.5, token.width * 0.33)),
                // Glyphs, NBSP, images and no-wrap text are unbreakable boxes.
                _ => items.push(Item::box_(token.width)),
            }
        }
        items.push(Item::glue(0.0, 1.0e9, 0.0));
        items.push(Item::forced_break());
        items
    }
}

#[cfg(test)]
mod tests {
    use crate::LaidOutDocument;
    use crate::parser::DocumentFactory;
    use crate::test_support::TestGlyphShaper as GlyphCache;
    use lightningcss::stylesheet::ParserOptions;
    use std::fs;

    fn layout_html(html: &str, width: f64) -> LaidOutDocument {
        let mut factory = DocumentFactory::new();
        let mut glyph_cache = GlyphCache::new();
        factory.parse_with_new_pipeline(html, None).shape(&mut glyph_cache).expect("test shaper must register every glyph").layout(crate::LayoutConstraints::new(width, 16.0).unwrap())
    }

    fn line_text(document: &LaidOutDocument, line_idx: usize) -> String {
        let line = document.render_view().line(line_idx).expect("line index in range");
        document.glyph_slice(line.glyphs()).iter().map(|&glyph| document.glyph_metrics().get(glyph).ch()).collect()
    }

    fn tab_advance(document: &LaidOutDocument, line_idx: usize) -> f64 {
        document.render_view().line_glyph_advances(line_idx).expect("advance list for line").iter().next().expect("preserved tab advance").advance()
    }

    #[test]
    fn preserved_tab_advances_to_the_next_number_based_stop() {
        let document = layout_html("<html><body><pre style='tab-size: 4'>a\tb</pre></body></html>", 300.0);
        assert_eq!(line_text(&document, 0), "a\tb");
        // Test shaper: every 16px glyph/space advances 8px. After `a`, a
        // four-space tab interval reaches the 32px stop, hence 24px.
        assert!((tab_advance(&document, 0) - 24.0).abs() < 0.001);
    }

    #[test]
    fn number_based_tab_uses_the_nearest_block_space_metric() {
        let document = layout_html("<html><body><pre style='tab-size: 4'>a<span style='font-size: 32px'>\t</span>b</pre></body></html>", 300.0);
        assert!((tab_advance(&document, 0) - 24.0).abs() < 0.001, "the inline span's larger font must not redefine the block's tab stops");
    }

    #[test]
    fn tab_stops_are_anchored_at_the_block_content_edge_before_indent() {
        let document = layout_html("<html><body><pre style='tab-size: 4; text-indent: 8px'>a\tb</pre></body></html>", 300.0);
        assert!((tab_advance(&document, 0) - 16.0).abs() < 0.001);
    }

    #[test]
    fn length_and_zero_tab_sizes_produce_exact_sparse_advances() {
        let length = layout_html("<html><body><pre style='tab-size: 20px'>a\tb</pre></body></html>", 300.0);
        assert!((tab_advance(&length, 0) - 12.0).abs() < 0.001);

        let zero = layout_html("<html><body><pre style='tab-size: 0'>a\tb</pre></body></html>", 300.0);
        assert_eq!(tab_advance(&zero, 0), 0.0);
    }

    #[test]
    fn collapsed_tabs_stay_on_the_ordinary_space_fast_path() {
        let document = layout_html("<html><body><p style='tab-size: 20px'>a\tb</p></body></html>", 300.0);
        assert_eq!(line_text(&document, 0), "a b");
        assert!(document.render_view().line_glyph_advances(0).expect("line advances").is_empty());
    }

    #[test]
    #[ignore]
    fn inspect_axel_chapter004_float_pipeline() {
        let html_path = std::env::var("CHAPTER_HTML_PATH").expect("set CHAPTER_HTML_PATH to the chapter fixture");
        let css_path = std::env::var("CHAPTER_CSS_PATH").expect("set CHAPTER_CSS_PATH to its stylesheet");

        fn find_dom_node_with_class(document: &crate::document::Document, node: crate::document::DomNodeId, class: &str) -> Option<crate::document::DomNodeId> {
            if document.dom_has_class(node, class) {
                return Some(node);
            }
            let element = document.element_ref(node)?;
            for child in element.children() {
                if let Some(found) = find_dom_node_with_class(document, child, class) {
                    return Some(found);
                }
            }
            None
        }

        let html = fs::read_to_string(&html_path).expect("chapter html");
        let mut factory = DocumentFactory::new();
        let mut document = factory.parse_to_dom(&html);
        document.set_root_font_size(crate::document::RootFontSize::new(16.0).unwrap());
        println!("step 2 dom: root={:?}", document.dom_root().map(|n| n.raw()));

        let author_css = fs::read_to_string(&css_path).expect("chapter css");
        let parsed_float_prop = lightningcss::properties::Property::parse_string(lightningcss::properties::PropertyId::from("float"), "left", ParserOptions::default()).expect("parse float property");
        let parsed_clear_prop = lightningcss::properties::Property::parse_string(lightningcss::properties::PropertyId::from("clear"), "both", ParserOptions::default()).expect("parse clear property");
        println!("step 2b parsed property float={parsed_float_prop:?} clear={parsed_clear_prop:?}");
        let styled = html_style::style_document(document, &[&author_css]);
        let (document, styles) = styled.into_parts();
        let prepared = crate::PreparedDocument::try_new(document, styles).expect("style resolver must produce complete styles for its document");
        let document = prepared.document();

        let dom_root = document.dom_root().expect("dom root");
        let first_letter_dom = find_dom_node_with_class(&document, dom_root, "first-letter").expect("first-letter DOM node");
        let first_letter_style_idx = prepared.styles().style_for_node(first_letter_dom).expect("first-letter style");
        let first_letter_style = prepared.style_view(first_letter_style_idx);
        println!(
            "step 3 style: float={:?} display={:?} font_size={} line_height={} margin_right={} margin_bottom={}",
            first_letter_style.float(),
            first_letter_style.display(),
            first_letter_style.font_size(),
            first_letter_style.line_height(),
            first_letter_style.margin_right().resolve(first_letter_style.font_size() as f64),
            first_letter_style.margin_bottom().resolve(first_letter_style.font_size() as f64),
        );
        assert!(matches!(first_letter_style.float(), crate::document::Float::Left | crate::document::Float::Right));

        let float_box_idx = (0..prepared.layout_tree().box_count())
            .find(|&i| prepared.layout_tree().get_box_dom_element(i).and_then(|node| document.node_id_from_raw(node)).is_some_and(|node| document.dom_has_class(node, "first-letter")))
            .expect("float box");
        let float_box = prepared.layout_tree().box_at(float_box_idx).expect("float box exists");
        let float_mode = match float_box.layout_mode() {
            crate::document::LayoutMode::Block(_) => "Block",
            crate::document::LayoutMode::Table(_) => "Table",
            crate::document::LayoutMode::TableRow(_) => "TableRow",
            crate::document::LayoutMode::TableCell(_) => "TableCell",
            crate::document::LayoutMode::Inline(_) => "Inline",
            crate::document::LayoutMode::Anonymous(_) => "Anonymous",
        };
        println!("step 4 box tree: float box mode={float_mode}");
        assert!(matches!(float_box.layout_mode(), crate::document::LayoutMode::Block(_)));

        let mut glyph_cache = GlyphCache::new();
        let shaped = prepared.shape(&mut glyph_cache).expect("test shaper must register every glyph");
        println!("step 5 shape: glyphs={}", glyph_cache.len());

        let laid_out = shaped.layout(crate::LayoutConstraints::new(220.0, 16.0).unwrap());
        println!("step 6 layout: lines={}", laid_out.line_count());

        let float_point = laid_out.get_box_point(float_box_idx);
        let float_size = laid_out.get_box_size(float_box_idx);
        for (idx, line) in laid_out.render_view().lines().iter().enumerate().take(20) {
            let text: String = laid_out.glyph_slice(line.glyphs()).iter().map(|&glyph| laid_out.glyph_metrics().get(glyph).ch()).collect();
            println!("line {idx}: point=({:.2},{:.2}) baseline={:.2} height={:.2} text={:?}", line.point().x, line.point().y, line.baseline(), line.height(), text);
        }
        let float_line = laid_out
            .render_view()
            .lines()
            .iter()
            .enumerate()
            .find(|(_, line)| {
                let text: String = laid_out.glyph_slice(line.glyphs()).iter().map(|&glyph| laid_out.glyph_metrics().get(glyph).ch()).collect();
                text.starts_with('E') || text.starts_with('R')
            })
            .map(|(idx, line)| (idx, line))
            .expect("line for float");
        println!("float box: point=({:.2},{:.2}) size=({:.2},{:.2})", float_point.x, float_point.y, float_size.width, float_size.height);
        println!("matched line {}: point=({:.2},{:.2}) baseline={:.2} height={:.2}", float_line.0, float_line.1.point().x, float_line.1.point().y, float_line.1.baseline(), float_line.1.height());
        assert!((float_point.y - float_line.1.point().y).abs() < 3.0, "float top should be near the first line top");
    }

    #[test]
    fn trims_collapsible_trailing_space_on_final_line() {
        let document = layout_html("<html><body><p>a </p></body></html>", 200.0);

        assert_eq!(document.line_count(), 1);
        assert_eq!(line_text(&document, 0), "a");
    }

    #[test]
    fn long_word_overflows_without_overflow_wrap() {
        let html = format!("<html><body><p>{}</p></body></html>", "a".repeat(40));
        let document = layout_html(&html, 100.0);

        assert_eq!(document.line_count(), 1, "an unbreakable word should overflow on one line by default");
    }

    #[test]
    fn overflow_wrap_breaks_long_word_without_losing_glyphs() {
        let html = format!("<html><body><p style=\"overflow-wrap: break-word\">{}</p></body></html>", "a".repeat(40));
        let document = layout_html(&html, 100.0);

        assert!(document.line_count() >= 2, "overflow-wrap should break the long word across lines");
        let combined: String = (0..document.line_count()).map(|i| line_text(&document, i)).collect();
        assert_eq!(combined, "a".repeat(40), "mid-word breaks must not drop glyphs");
    }

    #[test]
    fn overflow_wrap_prefers_space_break_over_mid_word_break() {
        // "aa bb": both words fit on a line, so overflow-wrap must wrap at the
        // space like normal wrapping, not mid-word.
        let html = "<html><body><p style=\"overflow-wrap: break-word\">aa bb</p></body></html>";
        let document = layout_html(html, 40.0);

        assert_eq!(document.line_count(), 2);
        assert_eq!(line_text(&document, 0), "aa");
        assert_eq!(line_text(&document, 1), "bb");
    }

    #[test]
    fn word_break_break_all_breaks_anywhere() {
        let html = format!("<html><body><p style=\"word-break: break-all\">{}</p></body></html>", "a".repeat(40));
        let document = layout_html(&html, 100.0);

        assert!(document.line_count() >= 2, "word-break: break-all should wrap the long word");
        let combined: String = (0..document.line_count()).map(|i| line_text(&document, i)).collect();
        assert_eq!(combined, "a".repeat(40));
    }

    #[test]
    fn nowrap_suppresses_overflow_wrap() {
        let html = format!("<html><body><p style=\"overflow-wrap: break-word; white-space: nowrap\">{}</p></body></html>", "a".repeat(40));
        let document = layout_html(&html, 100.0);

        assert_eq!(document.line_count(), 1, "white-space: nowrap forbids wrapping even with overflow-wrap");
    }

    #[test]
    fn small_inline_image_keeps_natural_size() {
        // A 16x16 image in a 600px column must not be enlarged to fill the column.
        let document = layout_html("<html><body><p><img src=\"x.png\" width=\"16\" height=\"16\"></p></body></html>", 600.0);

        let frag = document.render_view().all_image_fragments().iter().next().expect("expected an image fragment");
        assert_eq!(frag.size().width, 16.0);
    }

    #[test]
    fn oversized_inline_image_shrinks_to_available_width() {
        // A 2000x1000 image must shrink to fit the column (the exact content
        // width depends on UA margins), preserving its 2:1 aspect ratio.
        let document = layout_html("<html><body><p><img src=\"x.png\" width=\"2000\" height=\"1000\"></p></body></html>", 600.0);

        let frag = document.render_view().all_image_fragments().iter().next().expect("expected an image fragment");
        assert!(frag.size().width < 2000.0 && frag.size().width <= 600.0, "image should shrink to the column, got {}", frag.size().width);
        let ratio = frag.size().height / frag.size().width;
        assert!((ratio - 0.5).abs() < 0.01, "aspect ratio should be preserved (0.5), got {ratio}");
    }

    #[test]
    fn justified_paragraph_wraps_and_keeps_all_text() {
        let html = "<html><body><p style=\"text-align: justify\">one two three four five six seven eight nine ten</p></body></html>";
        let document = layout_html(html, 120.0);

        assert!(document.line_count() >= 2, "a narrow justified paragraph should wrap onto multiple lines");

        // Joining the per-line text (restoring the space discarded at each line
        // break) must reproduce the source in order with no glyph lost.
        let combined = (0..document.line_count()).map(|i| line_text(&document, i)).collect::<Vec<_>>().join(" ");
        let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(normalized, "one two three four five six seven eight nine ten");
    }

    #[test]
    fn floated_drop_cap_top_aligns_with_first_line() {
        let html = "<html><body><p><span style=\"float:left; font-size:450%; line-height:0.9em; margin-right:0.02em; margin-bottom:-0.25em;\">T</span>ext text text text</p></body></html>";
        let document = layout_html(html, 220.0);

        let line_idx = document
            .render_view()
            .lines()
            .iter()
            .enumerate()
            .find(|(_, line)| {
                let chars: String = document.glyph_slice(line.glyphs()).iter().map(|&glyph| document.glyph_metrics().get(glyph).ch()).collect();
                chars == "T"
            })
            .map(|(idx, _)| idx)
            .expect("drop cap line should exist");

        let line = document.render_view().line(line_idx).expect("line should exist");
        let glyph = document.glyph_slice(line.glyphs())[0];
        let baseline_offset = document.glyph_metrics().get(glyph).baseline_offset() as f64;

        assert!((line.baseline() - baseline_offset).abs() < 1.0, "drop cap baseline should align to glyph top");
    }

    #[test]
    fn preserves_pre_wrap_spaces_after_soft_wrap() {
        let html = "<html><body><p><span>LongLongLong</span><span style=\"white-space: pre-wrap\">  x</span></p></body></html>";

        let mut factory = DocumentFactory::new();
        let mut glyph_cache = GlyphCache::new();
        let document = factory.parse_with_new_pipeline(html, None).shape(&mut glyph_cache).expect("test shaper must register every glyph");

        let text: String = document.glyphs().iter().map(|&glyph| document.glyph_metrics().get(glyph).ch()).collect();
        let word_width: f64 = text.chars().zip(document.glyphs().iter()).take_while(|(c, _)| *c != ' ').map(|(_, &glyph)| document.glyph_metrics().get(glyph).advance() as f64).sum();

        let document = document.layout(crate::LayoutConstraints::new(word_width + 1.0, 16.0).unwrap());

        assert!(document.line_count() >= 2, "expected wrapping to produce multiple lines");
        assert_eq!(line_text(&document, 1), "  x");
    }

    #[test]
    fn wpt_break_spaces_preserves_segment_breaks() {
        // Semantic adaptation of WPT
        // css/css-text/white-space/break-spaces-newline-011.html. Visible
        // markers replace the reftest's colored boxes so this remains a
        // deterministic framework-neutral assertion:
        // https://github.com/web-platform-tests/wpt/blob/master/css/css-text/white-space/break-spaces-newline-011.html
        let document = layout_html("<html><body><div style=\"white-space: break-spaces\">A\nB</div></body></html>", 200.0);

        assert_eq!(document.line_count(), 2);
        assert_eq!(line_text(&document, 0), "A");
        assert_eq!(line_text(&document, 1), "B");
    }

    /// Benchmark greedy vs Knuth–Plass line breaking on the real EPUB chapters
    /// in `TestData`. Ignored by default; run with:
    ///   cargo test --release --lib bench_line_breaking -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_line_breaking() {
        use crate::parser::DocumentFactory;
        use crate::test_support::TestGlyphShaper as GlyphCache;
        use std::path::{Path, PathBuf};
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        const WIDTH: f64 = 600.0;
        const LINE_HEIGHT: f64 = 20.0;
        const ITERS: usize = 20;
        const MAX_FILES: usize = 60;
        // Force justification so the Knuth–Plass path engages on every paragraph.
        const JUSTIFY_CSS: &str = "* { text-align: justify; }";

        fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect(&path, out);
                } else if matches!(path.extension().and_then(|e| e.to_str()), Some("html" | "xhtml" | "htm")) {
                    out.push(path);
                }
            }
        }

        let test_data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../TestData");
        if !test_data.exists() {
            eprintln!("skipping: {} not found", test_data.display());
            return;
        }
        let mut files = Vec::new();
        collect(&test_data, &mut files);
        files.sort();
        files.truncate(MAX_FILES);

        let (mut total_glyphs, mut greedy_lines, mut knuth_lines) = (0usize, 0usize, 0usize);
        let (mut greedy_time, mut knuth_time) = (Duration::ZERO, Duration::ZERO);
        let mut fallbacks = 0usize;
        let mut counted = 0usize;

        for path in &files {
            let Ok(html) = std::fs::read_to_string(path) else { continue };
            let mut factory = DocumentFactory::new();
            let prepared = factory.parse_with_new_pipeline_css_chunks(&html, &[JUSTIFY_CSS]);
            let glyphs = prepared.inline_content().glyphs().len();
            if glyphs == 0 {
                continue;
            }

            let mut cache = GlyphCache::new();
            let document = prepared.shape(&mut cache).expect("test shaper must register every glyph");
            let (source, styles, mut layout_tree, inline_content, glyph_metrics) = document.layout_parts_mut();
            let mut layout_state = crate::document::LayoutState::default();

            // Greedy
            super::KP_TEST_OVERRIDE.store(0, Ordering::Relaxed);
            crate::layout::layout(source, styles, &mut layout_tree, inline_content, glyph_metrics, WIDTH, LINE_HEIGHT, &mut layout_state); // warm up
            let g_lines = layout_state.lines.len();
            let start = Instant::now();
            for _ in 0..ITERS {
                crate::layout::layout(source, styles, &mut layout_tree, inline_content, glyph_metrics, WIDTH, LINE_HEIGHT, &mut layout_state);
            }
            greedy_time += start.elapsed();

            // Knuth–Plass
            super::KP_TEST_OVERRIDE.store(1, Ordering::Relaxed);
            crate::layout::layout(source, styles, &mut layout_tree, inline_content, glyph_metrics, WIDTH, LINE_HEIGHT, &mut layout_state); // warm up
            let k_lines = layout_state.lines.len();
            let start = Instant::now();
            for _ in 0..ITERS {
                crate::layout::layout(source, styles, &mut layout_tree, inline_content, glyph_metrics, WIDTH, LINE_HEIGHT, &mut layout_state);
            }
            knuth_time += start.elapsed();
            super::KP_TEST_OVERRIDE.store(-1, Ordering::Relaxed);

            // A KP run that produced exactly the greedy line count on a wrapping
            // chapter most likely fell back; flag it as a rough signal.
            if k_lines == g_lines && g_lines > 3 {
                fallbacks += 1;
            }

            total_glyphs += glyphs;
            greedy_lines += g_lines;
            knuth_lines += k_lines;
            counted += 1;
        }

        let per = |d: Duration| d.as_secs_f64() * 1000.0 / (ITERS * counted.max(1)) as f64;
        println!("\n=== line-breaking benchmark (TestData) ===");
        println!("chapters: {counted}   glyphs: {total_glyphs}   iters/chapter: {ITERS}   width: {WIDTH}px");
        println!("greedy:  {:>8.3} ms total   {:>7.4} ms/chapter   lines: {greedy_lines}", greedy_time.as_secs_f64() * 1000.0, per(greedy_time));
        println!("knuth:   {:>8.3} ms total   {:>7.4} ms/chapter   lines: {knuth_lines}", knuth_time.as_secs_f64() * 1000.0, per(knuth_time));
        println!("knuth / greedy: {:.2}x", knuth_time.as_secs_f64() / greedy_time.as_secs_f64().max(f64::EPSILON));
        println!("chapters where KP == greedy line count (possible fallback): {fallbacks}/{counted}");
    }

    #[test]
    fn text_indent_percentage_resolves_against_containing_block_width() {
        // `text-indent: 50%` in a fixed 200px block indents the first line by
        // 100px (half the containing-block width), not by a fraction of the font
        // size. Lay out in a wide viewport so the inner div keeps its 200px width.
        let html = "<html><body style=\"margin:0\"><div style=\"width:200px\"><p style=\"margin:0; text-indent:50%\">hello world this is a longer line</p></div></body></html>";
        let document = layout_html(html, 600.0);

        let first = document.render_view().line(0).expect("line should exist");
        assert!((first.point().x - 100.0).abs() < 1.0, "expected first line indented ~100px (50% of 200px), got {}", first.point().x);
    }

    #[test]
    fn margin_left_percentage_resolves_against_containing_block_width() {
        // `margin-left: 25%` on a block resolves against its containing block's
        // width (200px) → 50px, regardless of font size.
        let html = "<html><body style=\"margin:0\"><div style=\"width:200px\"><p style=\"margin:0; margin-left:25%\">x</p></div></body></html>";
        let document = layout_html(html, 600.0);

        let first = document.render_view().line(0).expect("line should exist");
        assert!((first.point().x - 50.0).abs() < 1.0, "expected first line offset ~50px (25% of 200px), got {}", first.point().x);
    }
}
