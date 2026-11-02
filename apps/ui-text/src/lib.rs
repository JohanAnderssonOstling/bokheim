//! Shared English interface copy for the app and its browser startup screen.
//!
//! `text!(common::close)` expands to a string literal, so it also works in
//! constants, patterns, and standard formatting macros.

include!(concat!(env!("OUT_DIR"), "/text.rs"));
