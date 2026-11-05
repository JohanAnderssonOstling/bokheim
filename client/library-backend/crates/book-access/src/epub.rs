//! Shared sparse EPUB opening and prefetch policy.
use crate::{AsyncBookSource, SparseBookSource};
use book_content::EpubOpeningPlan;

/// Both native and browser readers use this complete HTML/CSS opening plan.
/// Optional bounded prefetch falls back to eager demand reads for large books.
pub async fn prepare_epub(source: SparseBookSource) -> std::io::Result<AsyncBookSource> {
    let source = AsyncBookSource::from(source);
    let reader_source = source.clone();
    let plan = EpubOpeningPlan::read(move || reader_source.reader()).await?;
    source.prefetch(plan.initial_ranges()?).await?;
    let reader_source = source.clone();
    let required = plan.required_ranges(move || reader_source.reader()).await?;
    source.prefetch(required).await?;
    Ok(source)
}
