/// A fold range representing a foldable code region.
///
/// The fold range spans from start_line to end_line (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FoldRange {
    /// Start line (inclusive)
    pub start_line: usize,
    /// End line (inclusive)
    pub end_line: usize,
}

impl FoldRange {
    pub fn new(start_line: usize, end_line: usize) -> Self {
        assert!(start_line <= end_line, "fold start_line must be <= end_line");
        Self { start_line, end_line }
    }
}
