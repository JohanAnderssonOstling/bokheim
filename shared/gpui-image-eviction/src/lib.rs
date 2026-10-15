//! Budgeted retention of decoded images and their GPUI atlas entries.

use gpui::{App, Global, ImageId, RenderImage};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

const IMAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;
const IMAGE_CACHE_TARGET_BYTES: usize = 48 * 1024 * 1024;

struct CachedImage {
    image: Arc<RenderImage>,
    bytes: usize,
    last_used: u64,
}

struct PaintedImages {
    images: HashMap<ImageId, CachedImage>,
    bytes: usize,
    budget: usize,
    target: usize,
    clock: u64,
    cleanup_scheduled: bool,
}

impl Default for PaintedImages {
    fn default() -> Self {
        Self::new(IMAGE_CACHE_BYTES, IMAGE_CACHE_TARGET_BYTES)
    }
}

impl PaintedImages {
    fn new(budget: usize, target: usize) -> Self {
        assert!(target <= budget);
        Self { images: HashMap::new(), bytes: 0, budget, target, clock: 0, cleanup_scheduled: false }
    }

    fn track(&mut self, image: &Arc<RenderImage>) -> bool {
        self.clock += 1;
        match self.images.entry(image.id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().last_used = self.clock;
                false
            }
            Entry::Vacant(entry) => {
                let bytes = (0..image.frame_count()).filter_map(|frame| image.as_bytes(frame)).map(<[u8]>::len).sum();
                self.bytes += bytes;
                entry.insert(CachedImage { image: image.clone(), bytes, last_used: self.clock });
                true
            }
        }
    }

    fn evict_unused(&mut self) -> Vec<Arc<RenderImage>> {
        if self.bytes <= self.budget {
            return Vec::new();
        }
        let mut unused: Vec<_> = self
            .images
            .iter()
            // The newest image may still be waiting for its first frame.
            // Keep it even when all older images have external owners.
            .filter(|(_, entry)| entry.last_used != self.clock && Arc::strong_count(&entry.image) == 1)
            .map(|(&id, entry)| (id, entry.last_used))
            .collect();
        unused.sort_unstable_by_key(|&(_, last_used)| last_used);
        let mut evicted = Vec::new();
        for (id, _) in unused {
            if self.bytes <= self.target {
                break;
            }
            let entry = self.images.remove(&id).expect("selected cached image");
            self.bytes -= entry.bytes;
            evicted.push(entry.image);
        }
        evicted
    }
}

impl Global for PaintedImages {}

/// Retain decoded pixels for reuse, both when installing prefetched images
/// and when painting directly with `Window::paint_image`.
///
/// Only this cache's final reference is evictable. Visible frames, page caches
/// and readers protect their images through ordinary Arc ownership. The byte
/// budget is soft while these owners remain. Unused pixels stay cached until
/// pressure from later image activity; there are no periodic cleanup tasks.
pub fn track_image(image: &Arc<RenderImage>, cx: &mut App) {
    let state = cx.default_global::<PaintedImages>();
    let inserted = state.track(image);
    if !inserted || state.bytes <= state.budget || state.cleanup_scheduled {
        return;
    }
    state.cleanup_scheduled = true;
    // Paint can borrow a window out of App. Defer and coalesce the pressure
    // sweep so atlas entries are removed from every window before pixels drop.
    cx.defer(|cx| {
        let state = cx.default_global::<PaintedImages>();
        state.cleanup_scheduled = false;
        let evicted = state.evict_unused();
        for image in evicted {
            cx.drop_image(image, None);
        }
    });
}

#[cfg(test)]
mod tests;
