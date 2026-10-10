//! Defines the [PdfContentMark] struct, a content marker capable of carrying metadata
//! that can be attached to one or more [PdfPageObject] objects to apply logical groupings
//! or organizational structure to a [PdfDocument].

use crate::{bindgen::FPDF_PAGEOBJECTMARK, pdfium::PdfiumLibraryBindingsAccessor};
use std::marker::PhantomData;

#[cfg(doc)]
use crate::pdf::document::{page::object::PdfPageObject, PdfDocument};

/// A content marker identifying one or more [PdfPageObject] objects as "elements of interest"
/// to a particular application or extension. Content markers can be used to attach metadata
/// to single objects or groups of objects, and to organize rendered page objects into
/// logical groupings or associations that may not necessarily be displayed during rendering
/// but may have special significance to a processing application.
///
/// More information on content markers and PDF's concept of "marked content" in general
/// in The PDF Reference, Sixth Edition, in Section 10.5, beginning on page 850.
pub struct PdfPageObjectMark<'a> {
    _handle: FPDF_PAGEOBJECTMARK,
    lifetime: PhantomData<&'a FPDF_PAGEOBJECTMARK>,
}

impl<'a> PdfiumLibraryBindingsAccessor<'a> for PdfPageObjectMark<'a> {}

#[cfg(feature = "thread_safe")]
unsafe impl<'a> Send for PdfPageObjectMark<'a> {}

#[cfg(feature = "thread_safe")]
unsafe impl<'a> Sync for PdfPageObjectMark<'a> {}
