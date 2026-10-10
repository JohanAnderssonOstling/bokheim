//! A connection owns its endpoint and its startup cancellation handle together.
pub struct Connection<T, C> {
    pub endpoint: T,
    generation: u64,
    startup: Option<C>,
}
impl<T, C> Connection<T, C> {
    pub fn new(endpoint: T, generation: u64, startup: C) -> Self {
        Self { endpoint, generation, startup: Some(startup) }
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
    pub fn waiting(&self, generation: u64) -> bool {
        self.accepts(generation) && self.startup.is_some()
    }
    pub fn ready(&mut self, generation: u64) {
        if self.accepts(generation) {
            self.startup.take();
        }
    }
}
