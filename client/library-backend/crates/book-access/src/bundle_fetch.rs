use std::io;

pub type BundleFetch = Box<dyn BundleFetcher>;

/// Synchronous parser access to the platform's async batch transport.
pub trait BundleFetcher: Send + Sync {
    fn fetch(&mut self, ranges: Vec<(u64, usize)>) -> io::Result<Vec<u8>>;
}

impl<F> BundleFetcher for F
where
    F: FnMut(Vec<(u64, usize)>) -> io::Result<Vec<u8>> + Send + Sync,
{
    fn fetch(&mut self, ranges: Vec<(u64, usize)>) -> io::Result<Vec<u8>> {
        self(ranges)
    }
}
