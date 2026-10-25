//! Geometry-based reading blocks for PDFs with an embedded text layer.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct PdfReadingFragment {
    pub text: String,
    /// Full-page coordinates, independent of zoom and margin cropping.
    pub rects: Vec<NormalizedRect>,
}

impl LocalDocumentSession {
    pub(super) fn extract_reading_fragments(&self, page_index: usize) -> PdfResult<Vec<PdfReadingFragment>> {
        let cached = self.analysis.borrow_mut().get(page_index);
        if let Some(analysis) = cached {
            return Ok(analysis.text.reading_fragments());
        }
        if let Some(source) = &self.page_source {
            source.prepare_page(page_index).map_err(PdfError::new)?;
        }
        let page = self.document.pages().get(page_index as i32).map_err(|error| PdfError::new(error.to_string()))?;
        self.page_analysis(&page, page_index).map(|analysis| analysis.text.reading_fragments())
    }
}

impl PdfPageTextLayout {
    /// Groups the embedded text layer by its visual lines. Untagged PDF
    /// semantics remain approximate: small bottom notes and obvious table rows
    /// are omitted, while ordinary prose is retained conservatively.
    pub fn reading_fragments(&self) -> Vec<PdfReadingFragment> {
        let characters = self.geometry.characters();
        let mut heights: Vec<_> = characters.iter().filter(|character| !character.character().is_whitespace()).map(|character| character.bounds().height()).filter(|height| *height > 0.0).collect();
        heights.sort_by(f32::total_cmp);
        let median_height = heights.get(heights.len() / 2).copied().unwrap_or(0.0);
        let body = PageTextGeometry::new(
            characters
                .iter()
                .filter(|character| {
                    let bounds = character.bounds();
                    character.character().is_whitespace() || !(bounds.top() > 0.75 && bounds.height() < median_height * 0.75)
                })
                .cloned(),
        );
        let lines = text_lines(&body);
        let table_rows = table_row_lines(&lines);
        let mut result: Vec<PdfReadingFragment> = Vec::new();
        let mut previous = None;
        for (index, line) in lines.iter().enumerate() {
            // Three separated text groups on the same row are likely columns
            // in a table. Two groups are retained for ordinary two-column prose.
            let toc_line = line.text.contains("...") && line.text.chars().last().is_some_and(|character| character.is_ascii_digit());
            if table_rows[index] || toc_line || line.text.starts_with("http://") || line.text.starts_with("https://") {
                previous = None;
                continue;
            }
            let continuation = previous.is_some_and(|bounds: NormalizedRect| {
                let gap = line.bounds.top() - bounds.bottom();
                line.bounds.top() > bounds.top() && gap >= -median_height * 0.3 && gap <= median_height * 0.8 && (line.bounds.left() - bounds.left()).abs() <= median_height * 2.0
            });
            if let Some(fragment) = result.last_mut().filter(|_| continuation) {
                fragment.text.push(' ');
                fragment.text.push_str(&line.text);
                fragment.rects.push(line.bounds);
            } else {
                result.push(PdfReadingFragment { text: line.text.clone(), rects: vec![line.bounds] });
            }
            previous = Some(line.bounds);
        }
        result
    }
}

/// Sweep vertical intervals so unrelated rows are never compared. Preserve the
/// existing per-line overlap rule, including mixed heights and two-column prose.
fn table_row_lines(lines: &[PageTextLine]) -> Vec<bool> {
    let mut order: Vec<_> = (0..lines.len()).collect();
    order.sort_by(|&left, &right| lines[left].bounds.top().total_cmp(&lines[right].bounds.top()));
    let mut active = Vec::<usize>::new();
    let mut overlaps = vec![1usize; lines.len()];
    for index in order {
        let bounds = lines[index].bounds;
        active.retain(|&other| lines[other].bounds.bottom() > bounds.top());
        for &other in &active {
            if overlaps[index] >= 3 && overlaps[other] >= 3 {
                continue;
            }
            let previous = lines[other].bounds;
            let overlap = bounds.bottom().min(previous.bottom()) - bounds.top().max(previous.top());
            if overlap > bounds.height().min(previous.height()) * 0.3 {
                overlaps[index] += 1;
                overlaps[other] += 1;
            }
        }
        active.push(index);
    }
    overlaps.into_iter().map(|count| count >= 3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_blocks_retain_page_coordinates_and_read_each_paragraph_once() {
        let characters: Vec<_> = "Readable paragraph".chars().enumerate().map(|(index, character)| TextCharacter::new(index, character, NormalizedRect::new(0.1 + index as f32 * 0.015, 0.2, 0.015, 0.02))).collect();
        let layout = PdfPageTextLayout { page_index: 0, geometry: PageTextGeometry::new(characters) };
        let fragments = layout.reading_fragments();
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].text, "Readable paragraph");
        assert!(!fragments[0].rects.is_empty());
        assert_eq!(fragments[0].rects[0].top(), 0.2);
    }

    #[test]
    fn obvious_small_bottom_notes_are_excluded() {
        let mut characters: Vec<_> = "Readable body paragraph".chars().enumerate().map(|(index, character)| TextCharacter::new(index, character, NormalizedRect::new(0.1 + index as f32 * 0.015, 0.2, 0.015, 0.02))).collect();
        let offset = characters.len();
        characters.extend("Small note".chars().enumerate().map(|(index, character)| TextCharacter::new(offset + index, character, NormalizedRect::new(0.1 + index as f32 * 0.01, 0.85, 0.01, 0.008))));
        let layout = PdfPageTextLayout { page_index: 0, geometry: PageTextGeometry::new(characters) };
        let fragments = layout.reading_fragments();
        assert_eq!(fragments.iter().map(|fragment| fragment.text.as_str()).collect::<Vec<_>>(), ["Readable body paragraph"]);
    }

    #[test]
    fn row_sweep_matches_the_overlap_rule_for_columns_and_mixed_heights() {
        let mut lines = Vec::new();
        for (top, height, columns) in [(0.1, 0.03, 2), (0.2, 0.02, 3), (0.25, 0.04, 1), (0.27, 0.015, 3), (0.5, 0.2, 1), (0.6, 0.01, 2)] {
            for column in 0..columns {
                lines.push(PageTextLine { text: format!("row {top} column {column}"), bounds: NormalizedRect::new(column as f32 * 0.3, top, 0.1, height) });
            }
        }
        lines.reverse();
        let expected: Vec<_> = lines
            .iter()
            .map(|line| {
                lines
                    .iter()
                    .filter(|other| {
                        let overlap = line.bounds.bottom().min(other.bounds.bottom()) - line.bounds.top().max(other.bounds.top());
                        overlap > line.bounds.height().min(other.bounds.height()) * 0.3
                    })
                    .count()
                    >= 3
            })
            .collect();
        assert_eq!(table_row_lines(&lines), expected);
    }
}
