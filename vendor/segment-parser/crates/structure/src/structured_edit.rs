use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredReplacement {
    base: SegmentTreeFingerprint,
    operations: Box<[StructuredEditOperation]>,
}

impl StructuredReplacement {
    fn new(base: &SegmentTree, operations: Vec<StructuredEditOperation>) -> Self { Self { base: base.fingerprint(), operations: operations.into_boxed_slice() } }
    fn combine(base: &SegmentTree, replacements: Vec<Self>) -> Option<Self> {
        let fingerprint = base.fingerprint();
        if replacements.is_empty() || replacements.iter().any(|replacement| replacement.base != fingerprint) {
            return None;
        }
        Some(Self::new(base, replacements.into_iter().flat_map(|replacement| replacement.operations.into_vec()).collect()))
    }
    pub(super) fn splice(base: &SegmentTree, range: Range<usize>, inserted: Vec<SegmentPiece>) -> Self {
        Self::new(base, vec![StructuredEditOperation::Splice {
            start: StructuredEditBoundary::at(base, range.start),
            end: StructuredEditBoundary::at(base, range.end),
            inserted: inserted.into_boxed_slice(),
        }])
    }
    fn replace_text(base: &SegmentTree, unit_id: TokenUnitId, range: Range<usize>, text: String) -> Self { Self::new(base, vec![StructuredEditOperation::ReplaceText { unit_id, range, text }]) }
    pub fn apply_to(&self, current: &SegmentTree) -> Option<SegmentTree> {
        if current.fingerprint() != self.base {
            return None;
        }
        let mut result = current.clone();
        self.operations.iter().try_for_each(|operation| operation.apply_to(&mut result).then_some(()))?;
        Some(result)
    }
    pub fn applied_text(&self, current: &SegmentTree) -> Option<String> { self.apply_to(current).map(|tree| tree.text().to_string()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructuredEditBoundary {
    Start,
    Before(TokenUnitId),
    End,
}

impl StructuredEditBoundary {
    fn at(tree: &SegmentTree, index: usize) -> Self {
        if index == 0 {
            return Self::Start;
        }
        tree.unit(index).map(|unit| Self::Before(unit.id())).unwrap_or(Self::End)
    }
    fn resolve(self, tree: &SegmentTree) -> Option<usize> {
        match self {
            Self::Start => Some(0),
            Self::Before(id) => tree.token_index(id),
            Self::End => Some(tree.unit_count()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StructuredEditOperation {
    ReplaceText { unit_id: TokenUnitId, range: Range<usize>, text: String },
    Splice { start: StructuredEditBoundary, end: StructuredEditBoundary, inserted: Box<[SegmentPiece]> },
}

impl StructuredEditOperation {
    fn apply_to(&self, tree: &mut SegmentTree) -> bool {
        match self {
            Self::ReplaceText { unit_id, range, text } => tree.token_index(*unit_id).is_some_and(|index| tree.replace_unit_text_range(index, range.clone(), text)),
            Self::Splice { start, end, inserted } => {
                let Some(start) = start.resolve(tree) else { return false; };
                let Some(end) = end.resolve(tree) else { return false; };
                if start > end {
                    return false;
                }
                tree.splice_unit_pieces(start..end, inserted.iter().cloned());
                true
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentPiece {
    content: SegmentPieceContent,
    token_id: Option<TokenUnitId>,
    semantic_id: Option<SemanticUnitId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SegmentPieceContent {
    Text(String),
    Tag(TagData),
    SemanticText { kind: UnitKind, text: String },
}

impl SegmentPiece {
    pub fn text(text: impl Into<String>) -> Self { Self { content: SegmentPieceContent::Text(text.into()), token_id: None, semantic_id: None } }
    pub fn tag(tag: TagData) -> Self { Self { content: SegmentPieceContent::Tag(tag), token_id: None, semantic_id: None } }
    pub fn semantic_text(kind: UnitKind, text: impl Into<String>) -> Self { assert!(kind.is_semantic_text(), "semantic text pieces require a semantic text unit kind"); Self { content: SegmentPieceContent::SemanticText { kind, text: text.into() }, token_id: None, semantic_id: None } }
    pub fn from_unit(unit: UnitRef<'_>) -> Self {
        let content = match unit.tag() {
            Some(tag) => SegmentPieceContent::Tag(tag.clone()),
            None if unit.kind().has_semantic_identity() => SegmentPieceContent::SemanticText { kind: unit.kind(), text: unit.text().to_string() },
            None => SegmentPieceContent::Text(unit.text().to_string()),
        };
        Self { content, token_id: Some(unit.id()), semantic_id: unit.semantic_id() }
    }
    /// Copies a unit's content and semantic kind while minting fresh IDs when inserted into another tree.
    pub fn detached_from_unit(unit: UnitRef<'_>) -> Self {
        let mut piece = Self::from_unit(unit);
        piece.token_id = None;
        piece.semantic_id = None;
        piece
    }
    pub fn text_content(&self) -> Option<&str> { match &self.content { SegmentPieceContent::Text(text) | SegmentPieceContent::SemanticText { text, .. } => Some(text), SegmentPieceContent::Tag(_) => None } }
    pub fn tag_data(&self) -> Option<&TagData> { match &self.content { SegmentPieceContent::Tag(tag) => Some(tag), SegmentPieceContent::Text(_) | SegmentPieceContent::SemanticText { .. } => None } }
    pub fn semantic_text_content(&self) -> Option<(UnitKind, &str)> { match &self.content { SegmentPieceContent::SemanticText { kind, text } => Some((*kind, text)), SegmentPieceContent::Text(_) | SegmentPieceContent::Tag(_) => None } }
    pub fn replacement_text_preserving_identity(&self, text: &str) -> Self {
        match &self.content {
            SegmentPieceContent::Text(_) => Self { content: SegmentPieceContent::Text(text.to_string()), token_id: self.token_id, semantic_id: None },
            SegmentPieceContent::SemanticText { kind, text: original } if original == text => Self { content: SegmentPieceContent::SemanticText { kind: *kind, text: text.to_string() }, token_id: self.token_id, semantic_id: self.semantic_id },
            SegmentPieceContent::SemanticText { .. } | SegmentPieceContent::Tag(_) => Self::text(text),
        }
    }
}

impl SegmentTree {
    pub fn replacement_for_unit_splice(&self, range: Range<usize>, inserted: impl IntoIterator<Item = SegmentPiece>) -> Option<StructuredReplacement> {
        (range.start <= range.end && range.end <= self.unit_count()).then(|| StructuredReplacement::splice(self, range, inserted.into_iter().collect()))
    }
    pub fn replacement_with_leading_whitespace(&self, whitespace: &str) -> StructuredReplacement { self.replacement_with_boundary_whitespace(whitespace, true) }
    pub fn replacement_with_trailing_whitespace(&self, whitespace: &str) -> StructuredReplacement { self.replacement_with_boundary_whitespace(whitespace, false) }
    fn replacement_with_boundary_whitespace(&self, whitespace: &str, leading: bool) -> StructuredReplacement {
        let boundary = self.boundary_non_tag_unit(leading);
        let insertion_index = boundary.map_or_else(|| if leading { self.unit_count() } else { 0 }, |(index, kind)| {
            if kind == UnitKind::Whitespace { index } else if leading { index } else { index + 1 }
        });
        let end = insertion_index + usize::from(boundary.is_some_and(|(_, kind)| kind == UnitKind::Whitespace));
        let inserted = (!whitespace.is_empty()).then(|| SegmentPiece::text(whitespace)).into_iter().collect();
        StructuredReplacement::splice(self, insertion_index..end, inserted)
    }
    fn boundary_non_tag_unit(&self, leading: bool) -> Option<(usize, UnitKind)> {
        if leading {
            return (0..self.unit_count()).find_map(|index| self.unit(index).filter(|unit| !unit.is_tag()).map(|unit| (index, unit.kind())));
        }
        (0..self.unit_count()).rev().find_map(|index| self.unit(index).filter(|unit| !unit.is_tag()).map(|unit| (index, unit.kind())))
    }
    pub fn rebuild(&mut self) {
        self.replace_preserving_inherited_formatting(build_tree_from_pieces(self.pieces_from_unit_range(0..self.units.len())));
    }
    pub fn splice_units(&mut self, range: Range<usize>, inserted: impl IntoIterator<Item = Run>) -> usize { let tokenized = tokenize_runs_into_parts(inserted.into_iter().collect()); self.splice_with_builder(range, |builder| builder.extend_tokenized(tokenized)) }
    pub fn replacement_for_text_span(&self, span: Range<usize>, text: &str) -> Option<StructuredReplacement> {
        if span.start > span.end || span.end > self.text.len() {
            return None;
        }
        if span.is_empty() {
            let insertion_index = self.units.partition_point(|unit| unit.end() <= span.start);
            let boundary_is_exact = self.units.get(insertion_index).is_some_and(|unit| unit.start() == span.start) || insertion_index == self.units.len() && span.start == self.text.len();
            if !boundary_is_exact || text.is_empty() {
                return None;
            }
            return Some(StructuredReplacement::splice(self, insertion_index..insertion_index, vec![SegmentPiece::text(text)]));
        }
        let matching = self.overlapping_unit_indices(&span);
        let first = self.unit(matching.start)?;
        let last = self.unit(matching.end.checked_sub(1)?)?;
        if matching.len() == 1 && !first.is_tag() {
            let unit_span = first.span();
            let local_span = span.start.checked_sub(unit_span.start)?..span.end.checked_sub(unit_span.start)?;
            if local_span.start > local_span.end || local_span.end > first.text().len() {
                return None;
            }
            return Some(StructuredReplacement::replace_text(self, first.id(), local_span, text.to_string()));
        }
        if first.span().start != span.start || last.span().end != span.end || matching.clone().filter_map(|index| self.unit(index)).any(|unit| unit.is_tag()) {
            return None;
        }
        Some(StructuredReplacement::splice(self, matching, vec![SegmentPiece::text(text)]))
    }
    pub fn replacement_for_text_spans(&self, replacements: &[(Range<usize>, String)]) -> Option<StructuredReplacement> {
        let mut ordered = replacements.to_vec();
        ordered.sort_by(|left, right| right.0.start.cmp(&left.0.start));
        if ordered.is_empty() || ordered.iter().any(|(span, _)| span.is_empty() || span.start > span.end || span.end > self.text.len()) {
            return None;
        }
        if ordered.windows(2).any(|pair| pair[1].0.end > pair[0].0.start) {
            return None;
        }
        let replacements = ordered.into_iter().map(|(span, text)| self.replacement_for_text_span(span, &text)).collect::<Option<Vec<_>>>()?;
        StructuredReplacement::combine(self, replacements)
    }
    fn overlapping_unit_indices(&self, span: &Range<usize>) -> Range<usize> {
        let start = self.units.partition_point(|unit| unit.end() <= span.start);
        start..start + self.units[start..].iter().take_while(|unit| unit.start() < span.end).count()
    }
    pub fn replacement_for_whitespace_fact(&self, fact: WhitespaceTypographyFact) -> Option<StructuredReplacement> {
        let span = fact.span();
        let preferred = fact.preferred_text()?;
        let observed = self.text.get(span.clone()).map(WhitespaceTypographyKind::from_text)?;
        if observed != fact.observed() || observed.text() == Some(preferred) {
            return None;
        }
        if let Some(side) = fact.punctuation_side() {
            return self.replacement_for_punctuation_spacing(fact.subject_span(), side, preferred);
        }
        if !span.is_empty() {
            return self.replacement_for_text_span(span, preferred);
        }
        self.replacement_for_text_span(span.clone(), preferred).or_else(|| self.replacement_inside_subject(span.start, fact.subject_span(), preferred))
    }
    pub fn replacement_for_whitespace_facts(&self, facts: &[WhitespaceTypographyFact]) -> Option<StructuredReplacement> {
        let replacements = facts.iter().copied().map(|fact| self.replacement_for_whitespace_fact(fact)).collect::<Option<Vec<_>>>()?;
        StructuredReplacement::combine(self, replacements)
    }
    pub fn replacement_for_punctuation_spacing_pair(&self, punctuation_span: Range<usize>, before_span: Range<usize>, after_span: Range<usize>, before: Option<&str>, after: Option<&str>) -> Option<StructuredReplacement> {
        let replacements = [(PunctuationSpacingSide::Before, before_span, before), (PunctuationSpacingSide::After, after_span, after)].into_iter()
            .filter_map(|(side, span, whitespace)| whitespace.and_then(|whitespace| self.replacement_for_punctuation_spacing_span(punctuation_span.clone(), span, side, whitespace)))
            .collect::<Vec<_>>();
        StructuredReplacement::combine(self, replacements)
    }
    fn replacement_for_punctuation_spacing_span(&self, punctuation_span: Range<usize>, span: Range<usize>, side: PunctuationSpacingSide, whitespace: &str) -> Option<StructuredReplacement> {
        if self.text.get(span.clone()) == Some(whitespace) {
            return None;
        }
        if !span.is_empty() {
            return self.replacement_for_text_span(span, whitespace);
        }
        self.replacement_for_punctuation_spacing(punctuation_span, side, whitespace)
    }
    fn replacement_inside_subject(&self, position: usize, subject_span: Range<usize>, text: &str) -> Option<StructuredReplacement> {
        if text.is_empty() || position < subject_span.start || position > subject_span.end {
            return None;
        }
        let unit = self.iter_units().find(|unit| !unit.is_tag() && unit.span().start < position && position < unit.span().end && subject_span.start <= unit.span().start && unit.span().end <= subject_span.end)?;
        let local = position.checked_sub(unit.span().start)?;
        Some(StructuredReplacement::replace_text(self, unit.id(), local..local, text.to_string()))
    }
    fn replacement_for_punctuation_spacing(&self, punctuation_span: Range<usize>, side: PunctuationSpacingSide, whitespace: &str) -> Option<StructuredReplacement> {
        let punctuation_index = match side {
            PunctuationSpacingSide::Before => self.iter_units().position(|unit| unit.kind() == UnitKind::Punct && unit.span().start == punctuation_span.start),
            PunctuationSpacingSide::After => self.iter_units().rposition(|unit| unit.kind() == UnitKind::Punct && unit.span().end == punctuation_span.end),
        }?;
        let adjacent_index = match side {
            PunctuationSpacingSide::Before => punctuation_index.checked_sub(1),
            PunctuationSpacingSide::After => punctuation_index.checked_add(1).filter(|index| *index < self.unit_count()),
        };
        if let Some(adjacent) = adjacent_index.and_then(|index| self.unit(index)).filter(|unit| unit.kind() == UnitKind::Whitespace) {
            return self.replacement_for_text_span(adjacent.span(), whitespace);
        }
        if whitespace.is_empty() {
            return None;
        }
        let insertion_index = match side {
            PunctuationSpacingSide::Before => punctuation_index,
            PunctuationSpacingSide::After => punctuation_index + 1,
        };
        Some(StructuredReplacement::splice(self, insertion_index..insertion_index, vec![SegmentPiece::text(whitespace)]))
    }
    pub fn replace_unit_text_range(&mut self, unit_index: usize, range: Range<usize>, text: &str) -> bool {
        let Some(unit) = self.unit(unit_index) else { return false; };
        if unit.is_tag() {
            return false;
        }
        let unit_text = unit.text().to_string();
        if range.start > range.end || range.end > unit_text.len() {
            return false;
        }
        let mut new_text = String::with_capacity(unit_text.len() + text.len());
        new_text.push_str(&unit_text[..range.start]);
        new_text.push_str(text);
        new_text.push_str(&unit_text[range.end..]);
        let replacement = self.piece_from_unit(unit).replacement_text_preserving_identity(&new_text);
        self.splice_unit_pieces(unit_index..unit_index + 1, [replacement]);
        true
    }
    pub fn replace_text(&mut self, query: &str, replacement: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let mut count = 0usize;
        let pieces = self.iter_units().map(|unit| match unit.tag() {
            Some(_) => SegmentPiece::from_unit(unit),
            None => {
                let matches = unit.text().matches(query).count();
                count += matches;
                if matches > 0 { SegmentPiece::text(unit.text().replace(query, replacement)) } else { self.piece_from_unit(unit) }
            }
        }).collect::<Vec<_>>();
        if count > 0 {
            self.replace_preserving_inherited_formatting(build_tree_from_pieces(pieces));
        }
        count
    }
    pub fn splice_unit_pieces(&mut self, range: Range<usize>, inserted: impl IntoIterator<Item = SegmentPiece>) -> usize { self.splice_with_builder(range, |builder| builder.extend(inserted)) }
    fn splice_with_builder(&mut self, range: Range<usize>, insert: impl FnOnce(&mut PieceTreeBuilder)) -> usize {
        let mut builder = PieceTreeBuilder::default();
        builder.extend_tree_range(self, 0..range.start);
        let before_inserted = builder.unit_count();
        insert(&mut builder);
        let inserted_len = builder.unit_count() - before_inserted;
        if range.is_empty() && inserted_len == 0 { return 0; }
        builder.extend_tree_range(self, range.end..self.units.len());
        self.replace_preserving_inherited_formatting(builder.finish());
        inserted_len
    }
    fn replace_preserving_inherited_formatting(&mut self, mut replacement: SegmentTree) {
        replacement.inherited_formatting_counts = self.inherited_formatting_counts;
        replacement.copy_inline_placeables_preserved_by(self);
        *self = replacement;
    }
    pub(super) fn piece_from_unit(&self, unit: UnitRef<'_>) -> SegmentPiece { SegmentPiece::from_unit(unit) }
    pub fn pieces_from_unit_range(&self, range: Range<usize>) -> Vec<SegmentPiece> { range.filter_map(|index| self.unit(index)).map(|unit| self.piece_from_unit(unit)).collect() }
}

#[derive(Default)]
struct PieceTreeBuilder {
    text: String,
    units: Vec<TokenUnit>,
    tags: Vec<TagData>,
    semantic_ids: Vec<Option<SemanticUnitId>>,
}

impl PieceTreeBuilder {
    fn push(&mut self, piece: SegmentPiece) {
        match piece.content {
            SegmentPieceContent::Text(value) => self.push_text(value, piece.token_id),
            SegmentPieceContent::Tag(tag) => self.push_tag(tag, piece.token_id, piece.semantic_id),
            SegmentPieceContent::SemanticText { kind, text } => self.push_semantic_text(kind, text, piece.token_id, piece.semantic_id),
        }
    }
    fn extend(&mut self, pieces: impl IntoIterator<Item = SegmentPiece>) { pieces.into_iter().for_each(|piece| self.push(piece)); }
    fn extend_tree_range(&mut self, tree: &SegmentTree, range: Range<usize>) {
        if range.is_empty() { return; }
        let source_start = tree.units.get(range.start).map(TokenUnit::start).unwrap_or(tree.text.len());
        let source_end = tree.units.get(range.end).map(TokenUnit::start).unwrap_or(tree.text.len());
        let destination_start = self.text.len();
        self.text.push_str(tree.text.get(source_start..source_end).expect("unit range must address valid text boundaries"));
        range.for_each(|unit_index| {
            let unit = &tree.units[unit_index];
            let tag_index = unit.tag_index().map(|tag_index| {
                let destination_tag_index = u16::try_from(self.tags.len()).expect("tag count exceeds u16 range");
                self.tags.push(tree.tags[tag_index].clone());
                destination_tag_index
            });
            self.units.push(TokenUnit::new_compact(unit.kind(), destination_start + unit.start() - source_start, destination_start + unit.end() - source_start, tag_index).with_token_id(unit.token_id()));
            self.semantic_ids.push(tree.unit_semantic_id(unit_index));
        });
    }
    fn extend_tokenized(&mut self, tokenized: TokenizedRun) {
        let (text, units, tags, _) = tokenized.into_parts();
        let source_start = self.text.len();
        let first_tag_index = self.tags.len();
        let unit_count = units.len();
        self.text.push_str(&text);
        self.tags.extend(tags);
        self.units.extend(units.into_iter().map(|unit| {
            let tag_index = unit.tag_index().map(|index| u16::try_from(first_tag_index + index).expect("tag count exceeds u16 range"));
            TokenUnit::new_compact(unit.kind(), source_start + unit.start(), source_start + unit.end(), tag_index)
        }));
        self.semantic_ids.extend((0..unit_count).map(|_| None));
    }
    fn unit_count(&self) -> usize { self.units.len() }
    fn finish(self) -> SegmentTree { SegmentTree::finish_build(SegmentTree::from_tokenized_parts_with_semantic_ids(self.text, self.units, self.tags, Vec::new(), Some(self.semantic_ids))) }
    fn push_text(&mut self, value: String, token_id: Option<TokenUnitId>) {
        let start = self.text.len();
        let first_unit = self.units.len();
        atomize_text_into(&value, start, &mut self.units);
        self.semantic_ids.extend((first_unit..self.units.len()).map(|_| None));
        if let (Some(id), Some(unit)) = (token_id, (first_unit < self.units.len()).then(|| self.units.last_mut()).flatten()) { unit.set_token_id(id); }
        self.text.push_str(&value);
    }
    fn push_tag(&mut self, tag: TagData, token_id: Option<TokenUnitId>, semantic_id: Option<SemanticUnitId>) {
        let tag_index = u16::try_from(self.tags.len()).expect("tag count exceeds u16 range");
        self.tags.push(tag);
        self.units.push(TokenUnit::new_compact(UnitKind::Tag, self.text.len(), self.text.len(), Some(tag_index)).with_token_id(token_id));
        self.semantic_ids.push(semantic_id);
    }
    fn push_semantic_text(&mut self, kind: UnitKind, value: String, token_id: Option<TokenUnitId>, semantic_id: Option<SemanticUnitId>) {
        if value.is_empty() { return; }
        let start = self.text.len();
        self.text.push_str(&value);
        self.units.push(TokenUnit::new_compact(kind, start, self.text.len(), None).with_token_id(token_id));
        self.semantic_ids.push(semantic_id);
    }
}

fn build_tree_from_pieces(pieces: Vec<SegmentPiece>) -> SegmentTree { let mut builder = PieceTreeBuilder::default(); builder.extend(pieces); builder.finish() }

#[cfg(test)]
mod tests {
    use super::*;

    fn terminology_tree() -> SegmentTree {
        let text = "Mandubulär framdragning med ocklusala block.";
        let terms = ["Mandubulär", "ocklusala"];
        let spans = terms.into_iter().filter_map(|term| text.find(term).map(|start| (UnitKind::Term, start..start + term.len()))).collect::<Vec<_>>();
        SegmentTree::from_runs_with_resolved_spans([Run::new(text)], &spans)
    }

    fn whitespace_fact(observed: WhitespaceTypographyKind, role: WhitespaceTypographyRole, preferred: WhitespaceTypographyKind, span: Range<usize>, subject_span: Range<usize>) -> WhitespaceTypographyFact { WhitespaceTypographyFact::new(observed, role, Some(preferred), WhitespaceTypographyAssessment::Foreign, span, subject_span) }

    #[test]
    fn whitespace_fact_replaces_and_removes_existing_separator() {
        let base = SegmentTree::from_text("50 %");
        let replacement = whitespace_fact(WhitespaceTypographyKind::Regular, WhitespaceTypographyRole::PercentSeparator, WhitespaceTypographyKind::NonBreaking, 2..3, 0..4);
        let removal = whitespace_fact(WhitespaceTypographyKind::Regular, WhitespaceTypographyRole::PercentSeparator, WhitespaceTypographyKind::Absent, 2..3, 0..4);

        assert_eq!(base.replacement_for_whitespace_fact(replacement).and_then(|edit| edit.applied_text(&base)), Some("50\u{00A0}%".to_string()));
        assert_eq!(base.replacement_for_whitespace_fact(removal).and_then(|edit| edit.applied_text(&base)), Some("50%".to_string()));
    }

    #[test]
    fn whitespace_fact_inserts_inside_atomic_measurement() {
        let base = SegmentTree::from_tokenized_parts_with_semantic_ids("5mg".to_string(), vec![TokenUnit::new(UnitKind::Measurement, 0..3)], Vec::new(), Vec::new(), None);
        let fact = whitespace_fact(WhitespaceTypographyKind::Absent, WhitespaceTypographyRole::MeasurementSeparator, WhitespaceTypographyKind::NonBreaking, 1..1, 0..3);

        assert_eq!(base.replacement_for_whitespace_fact(fact).and_then(|edit| edit.applied_text(&base)), Some("5\u{00A0}mg".to_string()));
    }

    #[test]
    fn punctuation_whitespace_fact_inserts_before_zero_width_tag() {
        let mut run = Run::new("Hello,world");
        run.push_tag_at(6, TagData::new("1".to_string(), "x".to_string(), TagKind::Standalone, "<x/>".to_string(), None, None));
        let base = SegmentTree::from_runs([run]);
        let fact = whitespace_fact(WhitespaceTypographyKind::Absent, WhitespaceTypographyRole::AfterPunctuation, WhitespaceTypographyKind::Regular, 6..6, 5..6);
        let applied = base.replacement_for_whitespace_fact(fact).and_then(|edit| edit.apply_to(&base)).expect("tag-aware whitespace insertion");
        let kinds = applied.iter_units().map(|unit| unit.kind()).collect::<Vec<_>>();

        assert_eq!(applied.text(), "Hello, world");
        assert!(kinds.windows(3).any(|window| window == [UnitKind::Punct, UnitKind::Whitespace, UnitKind::Tag]));
    }

    #[test]
    fn punctuation_edit_preserves_unrelated_term_units_and_ids() {
        let base = terminology_tree();
        let terms = base.iter_units().filter(|unit| unit.kind() == UnitKind::Term).map(|unit| (unit.text().to_string(), unit.id())).collect::<Vec<_>>();
        let period = base.text().len() - 1..base.text().len();
        let edit = base.replacement_for_text_span(period, ",").expect("punctuation edit");
        let applied = edit.apply_to(&base).expect("fresh edit");

        assert_eq!(applied.text(), "Mandubulär framdragning med ocklusala block,");
        assert_eq!(applied.iter_units().filter(|unit| unit.kind() == UnitKind::Term).map(|unit| (unit.text().to_string(), unit.id())).collect::<Vec<_>>(), terms);
    }

    #[test]
    fn structured_edit_rejects_a_stale_base_tree() {
        let base = terminology_tree();
        let edit = base.replacement_for_text_span(base.text().len() - 1..base.text().len(), ",").expect("punctuation edit");
        let changed = SegmentTree::from_text("Changed target");

        assert!(edit.apply_to(&changed).is_none());
    }

    #[test]
    fn repeated_materialization_has_the_same_structural_fingerprint() {
        let base = terminology_tree();
        let edit = base.replacement_for_text_span(base.text().len() - 1..base.text().len(), ",").expect("punctuation edit");
        let preview = edit.apply_to(&base).expect("preview");
        let applied = edit.apply_to(&base).expect("application");

        assert_eq!(preview.fingerprint(), applied.fingerprint());
    }

    #[test]
    fn unit_splice_replacement_rejects_invalid_ranges() {
        let base = SegmentTree::from_text("Text");

        assert!(base.replacement_for_unit_splice(1..0, std::iter::empty::<SegmentPiece>()).is_none());
        assert!(base.replacement_for_unit_splice(0..base.unit_count() + 1, std::iter::empty::<SegmentPiece>()).is_none());
    }

    #[test]
    fn multiple_text_spans_in_one_unit_apply_from_right_to_left() {
        let tree = SegmentTree::from_text("a bc d");
        let replacement = tree.replacement_for_text_spans(&[(2..3, "B".to_string()), (3..4, "C".to_string())]).expect("replacement");
        assert_eq!(replacement.apply_to(&tree).expect("fresh replacement").text(), "a BC d");
    }

}
