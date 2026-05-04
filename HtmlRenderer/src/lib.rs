#![allow(non_snake_case)]

mod document;
mod layout;
mod parser;
mod renderer;
mod resources;
pub mod style;
mod text;

pub use epub_provider::TocEntry;
pub use renderer::HTMLRenderer;
