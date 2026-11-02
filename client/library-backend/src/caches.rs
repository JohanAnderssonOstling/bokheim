//! Cache maintenance in the process or browser worker that owns the retained bytes.
pub fn evict_cover_caches() {
    library_catalog::evict_cover_caches();
    #[cfg(not(target_arch = "wasm32"))]
    library_opds::evict_cover_cache();
}
