//! Minimal raw-WASM adapter for Bokheim's pure algorithms.
//!
//! Keeping the ABI to numbers and UTF-8 buffers avoids framework-generated JS
//! glue and makes the resulting module cacheable as a normal static asset.

use bokheim_algorithms::{valid_epub_cfi, valid_reading_position};

#[no_mangle]
pub extern "C" fn bokheim_alloc(length: usize) -> *mut u8 {
    let allocation = vec![0_u8; length].into_boxed_slice();
    Box::into_raw(allocation).cast::<u8>()
}

/// # Safety
///
/// `pointer` and `length` must describe a buffer returned by `bokheim_alloc`
/// that has not already been released.
#[no_mangle]
pub unsafe extern "C" fn bokheim_dealloc(pointer: *mut u8, length: usize) {
    let slice = std::ptr::slice_from_raw_parts_mut(pointer, length);
    drop(Box::from_raw(slice));
}

#[no_mangle]
pub extern "C" fn bokheim_valid_epub_cfi(pointer: *const u8, length: usize) -> u32 {
    with_utf8(pointer, length, valid_epub_cfi) as u32
}

#[no_mangle]
pub extern "C" fn bokheim_valid_reading_position(pointer: *const u8, length: usize) -> u32 {
    with_utf8(pointer, length, valid_reading_position) as u32
}

fn with_utf8(pointer: *const u8, length: usize, algorithm: impl FnOnce(&str) -> bool) -> bool {
    if pointer.is_null() || length > 8 * 1024 {
        return false;
    }
    // The JS adapter only passes allocations created by `bokheim_alloc`.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    std::str::from_utf8(bytes).is_ok_and(algorithm)
}
