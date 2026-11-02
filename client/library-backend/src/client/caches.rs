impl crate::LibraryClient {
    pub async fn evict_cover_caches(&self) -> Result<(), crate::BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::evict_cover_caches();
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        self.request(crate::LibraryCommand::Ready(crate::ReadyCommand::EvictCoverCaches)).await
    }
}
