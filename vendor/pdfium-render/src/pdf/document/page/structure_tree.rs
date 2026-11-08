//! Defines the [PdfStructureTree] struct, the root element of a tree of elements that
//! together describe the logical hierarchy of a single [PdfPage].

use crate::bindgen::{FPDF_PAGE, FPDF_STRUCTTREE};
use crate::pdfium::PdfiumLibraryBindingsAccessor;
use once_cell::sync::OnceCell;
use std::marker::PhantomData;

#[cfg(doc)]
use crate::pdf::document::page::PdfPage;

/// The root element of a tree of elements that together describe the logical hierarchy
/// of a single [PdfPage].
///
/// More information on the structure tree can be found in The PDF Reference, Sixth Edition,
/// in Section 10.6.1, beginning on page 856.
pub struct PdfPageStructureTree<'a> {
    page_handle: FPDF_PAGE,
    handle: OnceCell<FPDF_STRUCTTREE>,
    lifetime: PhantomData<&'a FPDF_STRUCTTREE>,
}

impl<'a> PdfPageStructureTree<'a> {
    pub(crate) fn from_pdfium(page_handle: FPDF_PAGE) -> Self {
        PdfPageStructureTree {
            page_handle,
            handle: OnceCell::new(),
            lifetime: PhantomData,
        }
    }

    /// Returns the internal `FPDF_STRUCTTREE` handle for this [PdfStructureTree].
    #[inline]
    pub(crate) fn handle(&self) -> FPDF_STRUCTTREE {
        *self
            .handle
            .get_or_init(|| unsafe { self.bindings().FPDF_StructTree_GetForPage(self.page_handle) })
    }

    pub(crate) fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            if !handle.is_null() {
                unsafe {
                    self.bindings().FPDF_StructTree_Close(handle);
                }
            }
        }
    }
}

impl<'a> Drop for PdfPageStructureTree<'a> {
    #[inline]
    fn drop(&mut self) {
        self.close();
    }
}

impl<'a> PdfiumLibraryBindingsAccessor<'a> for PdfPageStructureTree<'a> {}

#[cfg(feature = "thread_safe")]
unsafe impl<'a> Send for PdfPageStructureTree<'a> {}

#[cfg(feature = "thread_safe")]
unsafe impl<'a> Sync for PdfPageStructureTree<'a> {}
