//! Passive cover presentation shared by local and catalogue loaders.

use std::io::Cursor;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Div, Image, ImageFormat, SharedString};
use ui_components::BrowserTheme;

#[derive(Clone)]
pub(crate) enum BookCoverState {
    Loading,
    Loaded(Arc<Image>),
    Missing(SharedString),
}

pub(crate) fn book_cover(state: BookCoverState, theme: BrowserTheme) -> Div {
    match state {
        BookCoverState::Loading => ui_components::book_cover(theme),
        BookCoverState::Loaded(image) => ui_components::book_cover(theme).child(ui_components::book_cover_image(image, theme)),
        BookCoverState::Missing(label) => ui_components::missing_book_cover(label, theme),
    }
}

pub(crate) fn decode_book_cover_jpeg(bytes: Vec<u8>) -> Option<Arc<Image>> {
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    let (width, height) = image::io::Reader::with_format(Cursor::new(bytes.as_slice()), image::ImageFormat::Jpeg).into_dimensions().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)))
}
