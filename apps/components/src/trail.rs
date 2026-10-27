//! Folding a trail of crumbs — a folder path, a subject path — to one line.
//!
//! Crumbs are folded whole into a single "…", never cut: a crumb with an
//! ellipsis in it names nothing, and one "…" says plainly that something
//! is left out.

use std::ops::Range;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Refineable, Style, StyleRefinement, Styled, Window, point, size,
};

/// A trail on one line, folded to the width its parent gives it in the same
/// frame it is drawn: every crumb is laid out, and once the trail's own width
/// is known, [`fold_trail`] decides which of them are shown.
///
/// Crumbs must not shrink (`flex_none`): their laid-out widths are what the
/// fold is measured with. Separators are drawn between shown crumbs; the
/// fold, made for the run it stands for, takes the place of that run.
pub struct FoldedTrail {
    id: ElementId,
    style: StyleRefinement,
    crumbs: Vec<AnyElement>,
    separators: Vec<AnyElement>,
    fold: Box<dyn Fn(Range<usize>) -> AnyElement>,
    keep_end: bool,
}

/// A trail of `crumbs`, with `separator` between them and `fold` for a run
/// folded away. Lays out as a flex row: give it a width, as with `w_full`.
pub fn folded_trail(id: impl Into<ElementId>, crumbs: Vec<AnyElement>, separator: impl Fn() -> AnyElement, fold: impl Fn(Range<usize>) -> AnyElement + 'static) -> FoldedTrail {
    let separators = (1..crumbs.len()).map(|_| separator()).collect();
    FoldedTrail { id: id.into(), style: StyleRefinement::default(), crumbs, separators, fold: Box::new(fold), keep_end: false }.flex().flex_row().items_center().min_w_0().overflow_hidden()
}

impl FoldedTrail {
    /// When even the widest fold does not fit, clip the start rather than the
    /// end, so the last crumb stays in view.
    pub fn keep_end(mut self) -> Self {
        self.keep_end = true;
        self
    }
}

impl Styled for FoldedTrail {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// What prepaint placed, in the order it is painted.
pub enum TrailPart {
    Crumb(usize),
    Separator(usize),
    Fold,
}

impl Element for FoldedTrail {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = (Vec<TrailPart>, Option<AnyElement>);

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let children = self.crumbs.iter_mut().chain(self.separators.iter_mut()).map(|child| child.request_layout(window, cx)).collect::<Vec<_>>();
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style, children.iter().copied(), cx), children)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, children: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        let count = self.crumbs.len();
        let crumbs = children[..count].iter().map(|id| window.layout_bounds(*id)).collect::<Vec<_>>();
        let separators = children[count..].iter().map(|id| window.layout_bounds(*id)).collect::<Vec<_>>();
        let separator_width = separators.first().map_or(Pixels::ZERO, |separator| separator.size.width);
        // The fold's width does not depend on the run it stands for. Measured
        // in a namespace of its own, so its ids are not the drawn fold's.
        let mut measured = (self.fold)(0..0);
        let fold_width = window.with_element_namespace("measure", |window| measured.layout_as_root(size(AvailableSpace::MaxContent, AvailableSpace::MaxContent), window, cx)).width;
        let width = |folded: &Range<usize>| {
            let items = folded_trail_items(count, folded);
            let shown = items.iter().map(|item| item.map_or(fold_width, |index| crumbs[index].size.width)).sum::<Pixels>();
            shown + separator_width * items.len().saturating_sub(1) as f32
        };
        let folded = fold_trail(count, |folded| width(folded) <= bounds.size.width).unwrap_or(0..0);
        let shown_width = width(&folded);
        let mut x = if self.keep_end && shown_width > bounds.size.width { bounds.right() - shown_width } else { bounds.left() };
        let mut parts = Vec::new();
        let mut fold = None;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (position, item) in folded_trail_items(count, &folded).into_iter().enumerate() {
                if position > 0 {
                    let index = position - 1;
                    window.with_element_offset(point(x - separators[index].origin.x, Pixels::ZERO), |window| self.separators[index].prepaint(window, cx));
                    parts.push(TrailPart::Separator(index));
                    x += separator_width;
                }
                match item {
                    Some(index) => {
                        window.with_element_offset(point(x - crumbs[index].origin.x, Pixels::ZERO), |window| self.crumbs[index].prepaint(window, cx));
                        parts.push(TrailPart::Crumb(index));
                        x += crumbs[index].size.width;
                    }
                    None => {
                        let mut element = (self.fold)(folded.clone());
                        let laid_out = element.layout_as_root(size(AvailableSpace::MaxContent, AvailableSpace::MaxContent), window, cx);
                        element.prepaint_at(point(x, bounds.top() + (bounds.size.height - laid_out.height) / 2.0), window, cx);
                        parts.push(TrailPart::Fold);
                        fold = Some(element);
                        x += laid_out.width;
                    }
                }
            }
        });
        (parts, fold)
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, (parts, fold): &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for part in parts.iter() {
                match part {
                    TrailPart::Crumb(index) => self.crumbs[*index].paint(window, cx),
                    TrailPart::Separator(index) => self.separators[*index].paint(window, cx),
                    TrailPart::Fold => {
                        if let Some(fold) = fold {
                            fold.paint(window, cx);
                        }
                    }
                }
            }
        });
    }
}

impl IntoElement for FoldedTrail {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The crumbs to fold so the trail fits, by `fits`: `None` when it fits whole.
///
/// Folding starts with the crumb nearest the middle and widens outwards,
/// alternating, so what is folded stays one run. The first crumb goes only
/// once everything between it and the last is folded. The last is never
/// folded: it is where the trail leads. When even that does not fit, the
/// widest fold is returned and the trail is clipped.
pub fn fold_trail(count: usize, mut fits: impl FnMut(&Range<usize>) -> bool) -> Option<Range<usize>> {
    if count < 2 || fits(&(0..0)) {
        return None;
    }
    let middle = (count - 1) as f32 / 2.0;
    let mut order = (1..count - 1).collect::<Vec<_>>();
    order.sort_by(|a, b| (*a as f32 - middle).abs().total_cmp(&(*b as f32 - middle).abs()).then(a.cmp(b)));
    order.push(0);
    let mut folded = 0..0;
    for taken in 1..=order.len() {
        let picked = &order[..taken];
        folded = picked.iter().copied().min().unwrap_or(0)..picked.iter().copied().max().unwrap_or(0) + 1;
        if fits(&folded) {
            break;
        }
    }
    Some(folded)
}

/// What a folded trail shows, in order: each crumb by its index, and the
/// folded run, wherever it is, as one `None`.
pub fn folded_trail_items(count: usize, folded: &Range<usize>) -> Vec<Option<usize>> {
    let mut items = Vec::with_capacity(count);
    for index in 0..count {
        if !folded.contains(&index) {
            items.push(Some(index));
        } else if index == folded.start {
            items.push(None);
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_widens_from_the_middle_then_takes_the_first_but_never_the_last() {
        // Seven crumbs ten wide, a separator of one, a fold of two.
        let width = |folded: &Range<usize>| {
            let items = folded_trail_items(7, folded);
            let crumbs = items.iter().map(|item| if item.is_some() { 10 } else { 2 }).sum::<usize>();
            crumbs + items.len() - 1
        };
        assert_eq!(fold_trail(7, |folded| width(folded) <= 76), None);
        assert_eq!(fold_trail(7, |folded| width(folded) <= 75), Some(3..4));
        assert_eq!(fold_trail(7, |folded| width(folded) <= 60), Some(2..4));
        assert_eq!(fold_trail(7, |folded| width(folded) <= 24), Some(1..6));
        assert_eq!(fold_trail(7, |folded| width(folded) <= 13), Some(0..6));
        assert_eq!(folded_trail_items(7, &(2..4)), vec![Some(0), Some(1), None, Some(4), Some(5), Some(6)]);
    }
}
