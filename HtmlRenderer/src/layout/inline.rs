// Inline layout for text and images.
// `layout_runs` now coordinates an explicit pipeline instead of mixing all phases in one loop.
// Call flow: `layout_runs` -> `build_inline_tokens` -> `break_lines`
// -> `emit_lines`; emission uses `measure_line` -> `write_line_fragments`.
// Measurement uses `line_metrics_from_tokens`, `calculate_line_metrics_in_doc`,
// `compute_justification`, `compute_baseline`, and `vertical_align_offset`.
// Token building uses `effective_vertical_align` and `break_kind`.

use crate::document::{DocumentState, GlyphOffsetRun, ImageFragment, InlineKind, Line, TextAlign, VerticalAlignValue, WhiteSpace};
use crate::layout::{LayoutEngine, get_style};
use crate::text::glyph_cache::GlyphCache;
use floem::kurbo::{Point, Size};
use std::ops::Range;

/// Computes the extra word spacing for a justified line.
///
/// This is only used for pure text lines after line breaking has already chosen
/// the glyph span for the line.
fn compute_justification(doc: &DocumentState, cache: &GlyphCache, glyph_start: u32, glyph_end: u32, available_width: f64, is_last_line: bool, text_align: TextAlign, disable_justify: bool, force_justify: bool) -> (f64, f64) {
    if is_last_line || disable_justify || !(text_align == TextAlign::Justify || (force_justify && text_align == TextAlign::Left)) {
        return (0.0, 0.0);
    }

    let content_width: f64 = (glyph_start..glyph_end).map(|i| cache.get(doc.glyph_at(i as usize).unwrap_or_default()).size().width).sum();
    let extra_space = available_width - content_width;
    if extra_space <= 0.0 {
        return (0.0, 0.0);
    }

    let space_count = (glyph_start..glyph_end).filter(|&i| cache.get_char(doc.glyph_at(i as usize).unwrap_or_default()) == ' ').count();
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
fn effective_vertical_align(doc: &DocumentState, box_idx: usize) -> VerticalAlignValue {
    let style = get_style(doc, box_idx);
    if style.vertical_align() != VerticalAlignValue::Baseline {
        return style.vertical_align();
    }

    for ancestor_idx in doc.ancestors(box_idx) {
        let ancestor_style = get_style(doc, ancestor_idx);
        if ancestor_style.display() != crate::document::Display::Inline {
            break;
        }
        if ancestor_style.vertical_align() != VerticalAlignValue::Baseline {
            return ancestor_style.vertical_align();
        }
    }
    VerticalAlignValue::Baseline
}

struct InlineToken {
    kind: InlineTokenKind,
    width: f64,
    ascent: f64,
    descent: f64,
    font_size: f32,
    vertical_align: VerticalAlignValue,
    break_kind: BreakKind,
    white_space: WhiteSpace,
}

#[derive(Clone, Copy)]
enum InlineTokenKind {
    Glyph { glyph_idx: u32 },
    Image(u32),
    Break,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakKind {
    None,
    Soft,
    Hard,
}

struct BrokenLine {
    start: usize,
    end: usize,
    indent: f64,
    align: TextAlign,
    is_last_line: bool,
}

struct TokenPlacement {
    x: f64,
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
    line_width_limit: f64,
}

impl LayoutEngine {
    /// Lays out one inline run range from start to finish.
    ///
    /// This is the entry point used by box layout. It tokenizes the runs,
    /// breaks them into logical lines, and then emits fragments from those
    /// lines in a separate pass.
    pub(crate) fn layout_runs(&mut self, run_range: Range<u32>, position: Point, available_width: f64, text_indent: f64, text_indent_hanging: bool, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign) -> Size {
        if run_range.start >= run_range.end {
            return Size::ZERO;
        }
        let tokens = self.build_inline_tokens(run_range, (available_width - text_indent.max(0.0)).max(0.0));
        if tokens.is_empty() {
            return Size::ZERO;
        }
        let ctx = LineBreakContext { tokens: &tokens, available_width, text_indent, text_indent_hanging, position };
        let lines = self.break_lines(&ctx, text_indent_each_line, text_align, text_align_last);
        self.emit_lines(&ctx, &lines)
    }

    /// Breaks a token stream into logical lines without mutating the document.
    ///
    /// This keeps the line-breaking state machine separate from fragment emission
    /// and returns the spans plus per-line metadata needed by the emit phase.
    fn break_lines(&self, ctx: &LineBreakContext<'_>, text_indent_each_line: bool, text_align: TextAlign, text_align_last: TextAlign) -> Vec<BrokenLine> {
        let mut state = LineBreakState {
            x: 0.0,
            line_start: 0,
            last_break: None,
            is_first_line: true,
            line_indent: indent_for_line(ctx.text_indent, ctx.text_indent_hanging, true),
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
            // there is one; otherwise fall through and keep consuming so a too-long
            // word overflows rather than breaking mid-word. A super/sub marker is
            // kept attached to the preceding token instead of being stranded.
            if state.x + token.width > state.line_width_limit && i > state.line_start && !(state.last_break.is_none() && self.keep_with_previous_token(ctx, i)) {
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
            state.x += token.width;
            i += 1;
        }

        if state.line_start < ctx.tokens.len() {
            self.push_broken_line(&mut lines, ctx, &state, ctx.tokens.len(), text_align_last, true, false);
        }
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
            lines.push(BrokenLine { start: state.line_start, end: trimmed_end, indent: state.line_indent, align, is_last_line });
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
        state.line_width_limit = (ctx.available_width - state.line_indent).max(0.0);
    }

    /// Measures and emits broken lines, returning the final laid-out size.
    ///
    /// This is the write-side pass of the pipeline. It turns `BrokenLine`
    /// records into measured line layouts and then writes document fragments.
    fn emit_lines(&mut self, ctx: &LineBreakContext<'_>, lines: &[BrokenLine]) -> Size {
        let mut y = 0.0;
        let mut max_width = 0.0f64;
        for line in lines {
            let span = &ctx.tokens[line.start..line.end];
            let layout = self.measure_line(span, ctx.available_width - line.indent, line.is_last_line, line.align);
            max_width = max_width.max(layout.width + line.indent);
            y += self.write_line_fragments(span, line, y, ctx.position, &layout);
        }
        Size::new(max_width, y)
    }

    fn create_text_token(&self, glyph_idx: u32, style: crate::document::StyleView, white_space: WhiteSpace, vertical_align: VerticalAlignValue) -> InlineToken {
        let glyph_index = self.document.glyph_at(glyph_idx as usize).unwrap_or_default();
        let c = self.glyph_cache.get_char(glyph_index);

        let width = self.glyph_cache.get(glyph_index).size().width + style.letter_spacing() as f64 + if c == ' ' { style.word_spacing() as f64 } else { 0.0 };
        let (ascent, descent) = self.glyph_cache.glyph_metrics(glyph_index);
        let break_kind = self.break_kind(glyph_index, white_space);
        let font_size = style.font_size();
        InlineToken { kind: InlineTokenKind::Glyph { glyph_idx }, width, ascent, descent, font_size, vertical_align, break_kind, white_space }
    }

    fn create_image_token(&self, image_idx: &u32, style: crate::document::StyleView, vertical_align: VerticalAlignValue, max_width: f64) -> InlineToken {
        let (mut width, mut height) = self.document.images()[*image_idx as usize].display_size();
        if max_width > 0.0 && width > 0.0 {
            let scale = max_width / width;
            width = max_width;
            height = (height * scale).max(1.0);
        }
        let font_size = style.font_size();
        InlineToken { kind: InlineTokenKind::Image(*image_idx), width, ascent: height, descent: 0.0, font_size, vertical_align, break_kind: BreakKind::None, white_space: style.white_space() }
    }

    fn create_break_token(&self, style: crate::document::StyleView, vertical_align: VerticalAlignValue) -> InlineToken {
        InlineToken { kind: InlineTokenKind::Break, width: 0.0, ascent: 0.0, descent: 0.0, font_size: style.font_size(), vertical_align, break_kind: BreakKind::Hard, white_space: style.white_space() }
    }

    /// Converts inline runs into layout tokens.
    ///
    /// Each token carries the geometry and break metadata needed by the line
    /// breaker plus the placement inputs needed later by line measurement.

    fn build_inline_tokens(&self, run_range: Range<u32>, max_width: f64) -> Vec<InlineToken> {
        let mut tokens = Vec::new();
        for run in &self.document.inline_runs()[run_range.start as usize..run_range.end as usize] {
            let style = get_style(&self.document, run.box_idx as usize);
            let vertical_align = effective_vertical_align(&self.document, run.box_idx as usize);
            let white_space = style.white_space();
            match &run.kind {
                InlineKind::Text { glyphs } => tokens.extend(glyphs.clone().map(|i| self.create_text_token(i, style, white_space, vertical_align))),
                InlineKind::Image { image_idx } => {
                    tokens.push(self.create_image_token(image_idx, style, vertical_align, max_width));
                }
                InlineKind::Break => tokens.push(self.create_break_token(style, vertical_align)),
            }
        }
        tokens
    }

    /// Computes line metrics directly from token ascent and descent values.
    ///
    /// This is the fallback path for mixed-content lines where glyph-based text
    /// metrics are not used.
    fn line_metrics_from_tokens(&self, span: &[InlineToken]) -> (f64, f64) {
        let (max_ascent, max_descent) = span.iter().fold((0.0f64, 0.0f64), |(max_ascent, max_descent), token| (max_ascent.max(token.ascent), max_descent.max(token.descent)));
        compute_baseline(max_ascent, max_descent, self.line_height)
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
    fn measure_line(&self, span: &[InlineToken], line_width_limit: f64, is_last_line: bool, text_align: TextAlign) -> LineLayout {
        let summary = self.summarize_token_span(span);
        let (line_height, baseline, word_spacing, letter_spacing) = self.base_line_metrics(span, &summary, line_width_limit, is_last_line, text_align);
        let (line_height, baseline) = self.adjusted_line_metrics(span, line_height, baseline);
        let x_offset = self.line_x_offset(line_width_limit, summary.width, text_align);
        let placements = self.build_token_placements(span, line_height, baseline);

        LineLayout { glyph_range: summary.glyph_range, width: summary.width, x_offset, line_height, baseline, word_spacing, letter_spacing, placements }
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
    fn summarize_token_span(&self, span: &[InlineToken]) -> TokenSpanSummary {
        let (glyph_start, glyph_end, has_text, has_images, width) = span.iter().fold((u32::MAX, 0u32, false, false, 0.0f64), |(glyph_start, glyph_end, has_text, has_images, width), token| {
            (
                match token.kind {
                    InlineTokenKind::Glyph { glyph_idx } => glyph_start.min(glyph_idx),
                    InlineTokenKind::Image(_) | InlineTokenKind::Break => glyph_start,
                },
                match token.kind {
                    InlineTokenKind::Glyph { glyph_idx } => glyph_end.max(glyph_idx + 1),
                    InlineTokenKind::Image(_) | InlineTokenKind::Break => glyph_end,
                },
                has_text || matches!(token.kind, InlineTokenKind::Glyph { .. }),
                has_images || matches!(token.kind, InlineTokenKind::Image(_)),
                width + token.width,
            )
        });
        TokenSpanSummary { glyph_range: if has_text { glyph_start..glyph_end } else { 0..0 }, has_text, has_images, width }
    }

    /// Chooses the initial line metrics before vertical-align adjustments are applied.
    fn base_line_metrics(&self, span: &[InlineToken], summary: &TokenSpanSummary, line_width_limit: f64, is_last_line: bool, text_align: TextAlign) -> (f64, f64, f64, f64) {
        if summary.has_text && !summary.has_images {
            let (line_height, baseline) = self.calculate_line_metrics_in_doc(summary.glyph_range.start, summary.glyph_range.end);
            let disable_justify = span.iter().any(|token| matches!(token.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine));
            let (word_spacing, letter_spacing) = compute_justification(
                &self.document,
                &self.glyph_cache,
                summary.glyph_range.start,
                summary.glyph_range.end,
                line_width_limit,
                is_last_line,
                text_align,
                disable_justify,
                self.force_justify,
            );
            (line_height, baseline, word_spacing, letter_spacing)
        } else {
            let (line_height, baseline) = self.line_metrics_from_tokens(span);
            (line_height, baseline, 0.0, 0.0)
        }
    }

    /// Recomputes the line box after per-token vertical alignment offsets are applied.
    fn adjusted_line_metrics(&self, span: &[InlineToken], line_height: f64, baseline: f64) -> (f64, f64) {
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
    fn build_token_placements(&self, span: &[InlineToken], line_height: f64, baseline: f64) -> Vec<TokenPlacement> {
        let mut curr_x = 0.0;
        span.iter()
            .map(|token| {
                let placement = TokenPlacement { x: curr_x, y_offset: self.vertical_align_offset(token, line_height, baseline) as f32 };
                curr_x += token.width;
                placement
            })
            .collect()
    }

    /// Writes fragments for one broken line using a precomputed line layout.
    ///
    /// The write phase only mutates the document; all layout decisions have
    /// already been made by `break_lines` and `measure_line`.
    fn write_line_fragments(&mut self, span: &[InlineToken], broken_line: &BrokenLine, y: f64, position: Point, line: &LineLayout) -> f64 {
        let point = Point::new(position.x + broken_line.indent + line.x_offset, position.y + y);
        let layout = self.document.layout_mut();
        let line_idx = layout.lines.len();
        layout.lines.push(Line { glyphs: line.glyph_range.clone(), point, height: line.line_height, baseline: line.baseline, word_spacing: line.word_spacing, letter_spacing: line.letter_spacing });
        layout.line_glyph_offsets.push(Vec::new());

        for (token, placement) in span.iter().zip(&line.placements) {
            let offset = placement.y_offset as f64;
            if let InlineTokenKind::Image(image_idx) = token.kind {
                layout.image_fragments.push(ImageFragment { line_idx, image_idx, offset: Point::new(placement.x, line.baseline - token.ascent - offset), size: Size::new(token.width, token.ascent) });
            } else if let InlineTokenKind::Glyph { glyph_idx } = token.kind {
                Self::push_glyph_offset_run(&mut layout.line_glyph_offsets[line_idx], glyph_idx, offset as f32);
            }
        }
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

    /// Computes text-only line metrics from glyph metrics and overlapping styles.
    ///
    /// This path is used for text lines so font metrics and style line-height
    /// values both contribute to the final line box.
    fn calculate_line_metrics_in_doc(&self, start: u32, end: u32) -> (f64, f64) {
        let (max_ascent, max_descent) =
            (start..end).map(|i| self.glyph_cache.glyph_metrics(self.document.glyph_at(i as usize).unwrap_or_default())).fold((0.0f64, 0.0f64), |(max_ascent, max_descent), (ascent, descent)| (max_ascent.max(ascent), max_descent.max(descent)));

        let max_line_height = self.document.inline_runs().iter().fold(0.0f64, |max_height, run| {
            let InlineKind::Text { glyphs } = &run.kind else {
                return max_height;
            };
            if glyphs.end <= start || glyphs.start >= end {
                return max_height;
            }
            let style = get_style(&self.document, run.box_idx as usize);
            max_height.max(if style.line_height() > 0.0 { style.line_height() as f64 } else { style.font_size() as f64 * 1.2 })
        });

        compute_baseline(max_ascent, max_descent, max_line_height.max(self.line_height))
    }

    /// Classifies a glyph as a hard break, soft break, or non-break.
    ///
    /// This replaces the old pair of `breakable` and `hard_break` booleans.
    fn break_kind(&self, glyph_index: u16, white_space: WhiteSpace) -> BreakKind {
        match (white_space, self.glyph_cache.get_char(glyph_index)) {
            (WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine, '\n') => BreakKind::Hard,
            (WhiteSpace::NoWrap, _) | (_, '\u{00A0}') => BreakKind::None,
            (WhiteSpace::Pre, _) => BreakKind::None,
            (_, ' ' | '\t' | '\n') => BreakKind::Soft,
            _ => BreakKind::None,
        }
    }

    fn should_trim_leading_soft_token(&self, token: &InlineToken) -> bool {
        token.break_kind == BreakKind::Soft && !matches!(token.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap)
    }

    fn trim_trailing_line_whitespace(&self, ctx: &LineBreakContext<'_>, start: usize, end: usize) -> usize {
        let mut trimmed_end = end;
        while trimmed_end > start && self.should_trim_leading_soft_token(&ctx.tokens[trimmed_end - 1]) {
            trimmed_end -= 1;
        }
        trimmed_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::DocumentFactory;
    use crate::text::glyph_cache::GlyphCache;
    use crate::text::shaping::shape_document;

    fn layout_html(html: &str, width: f64) -> (DocumentState, GlyphCache) {
        let mut factory = DocumentFactory::new();
        let mut document = factory.parse_with_new_pipeline(html, None);
        let mut glyph_cache = GlyphCache::new();
        shape_document(&mut document, &mut glyph_cache);
        let mut engine = LayoutEngine::new(width, 16.0);
        engine.layout(document, glyph_cache)
    }

    fn line_text(document: &DocumentState, glyph_cache: &GlyphCache, line_idx: usize) -> String {
        let line = &document.lines()[line_idx];
        document.glyph_slice(line.glyphs.clone()).iter().map(|&glyph| glyph_cache.get_char(glyph)).collect()
    }

    #[test]
    fn trims_collapsible_trailing_space_on_final_line() {
        let (document, glyph_cache) = layout_html("<html><body><p>a </p></body></html>", 200.0);

        assert_eq!(document.line_count(), 1);
        assert_eq!(line_text(&document, &glyph_cache, 0), "a");
    }

    #[test]
    fn preserves_pre_wrap_spaces_after_soft_wrap() {
        let html = "<html><body><p><span>LongLongLong</span><span style=\"white-space: pre-wrap\">  x</span></p></body></html>";

        let mut factory = DocumentFactory::new();
        let mut document = factory.parse_with_new_pipeline(html, None);
        let mut glyph_cache = GlyphCache::new();
        shape_document(&mut document, &mut glyph_cache);

        let text: String = document.glyphs().iter().map(|&glyph| glyph_cache.get_char(glyph)).collect();
        let word_width: f64 = text.chars().zip(document.glyphs().iter()).take_while(|(c, _)| *c != ' ').map(|(_, &glyph)| glyph_cache.get(glyph).size().width).sum();

        let mut engine = LayoutEngine::new(word_width + 1.0, 16.0);
        let (document, glyph_cache) = engine.layout(document, glyph_cache);

        assert!(document.line_count() >= 2, "expected wrapping to produce multiple lines");
        assert_eq!(line_text(&document, &glyph_cache, 1), "  x");
    }
}
