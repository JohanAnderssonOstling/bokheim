use book_model::Annotation;
use serde::{Deserialize, Serialize};

/// One explicitly created library-wide annotation and its originating book's title.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecurringAnnotation {
    pub annotation: Annotation,
    pub book_title: String,
}
