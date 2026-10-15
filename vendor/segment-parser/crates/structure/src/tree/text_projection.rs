use std::ops::Range;

use super::SegmentTree;

impl SegmentTree {
    pub fn text_without_tags_for_span(&self, span: Range<usize>) -> String {
        self.iter_units()
            .filter(|unit| !unit.is_tag())
            .filter_map(|unit| {
                let unit_span = unit.span();
                let start = unit_span.start.max(span.start);
                let end = unit_span.end.min(span.end);
                (start < end).then(|| self.text.get(start..end)).flatten()
            })
            .collect()
    }
}
