//! GPUI UI for the embedded browser, with desktop and Android hosts.

#[cfg(any(target_os = "android", test))]
pub mod android;
