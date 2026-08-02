use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::{Arc, mpsc};
use std::thread;

use peniko::{Blob, Format, Image as PenikoImage};
use sha2::{Digest, Sha256};

use crate::document::{ImageResource, ImageSource};
use crate::resources::ResourceProvider;

pub struct DecodedImage {
    pub image: PenikoImage,
    pub hash: Vec<u8>,
    #[allow(dead_code)]
    pub byte_len: usize,
}

enum BytesRequest {
    Load(u32),
}

enum BytesResponse {
    Loaded(u32, Arc<Vec<u8>>),
    Failed(u32),
}

enum DecodeRequest {
    Decode(u32, Arc<Vec<u8>>),
}

enum DecodeResponse {
    Decoded(u32, DecodedImage),
    Failed(u32),
}

pub struct ImagePipeline {
    bytes_cache: HashMap<u32, Arc<Vec<u8>>>,
    decoded_cache: HashMap<u32, DecodedImage>,
    pending_bytes: HashSet<u32>,
    pending_decodes: HashSet<u32>,
    image_dimensions: HashMap<u32, (u32, u32)>,
    bytes_tx: mpsc::Sender<BytesRequest>,
    bytes_rx: mpsc::Receiver<BytesResponse>,
    decode_tx: mpsc::Sender<DecodeRequest>,
    decode_rx: mpsc::Receiver<DecodeResponse>,
}

#[derive(Debug, Default)]
pub struct ImagePipelinePoll {
    pub decoded: bool,
    pub dimensions_changed: Vec<u32>,
}

impl ImagePipeline {
    pub fn new(images: Arc<Vec<ImageResource>>, provider: Arc<dyn ResourceProvider>) -> Self {
        let (bytes_tx, bytes_rx_in) = mpsc::channel();
        let (bytes_tx_out, bytes_rx) = mpsc::channel();
        let images_clone = images.clone();
        let provider_clone = provider.clone();
        thread::spawn(move || bytes_worker(images_clone, provider_clone, bytes_rx_in, bytes_tx_out));

        let (decode_tx, decode_rx_in) = mpsc::channel();
        let (decode_tx_out, decode_rx) = mpsc::channel();
        thread::spawn(move || decode_worker(decode_rx_in, decode_tx_out));

        Self { bytes_cache: HashMap::new(), decoded_cache: HashMap::new(), pending_bytes: HashSet::new(), pending_decodes: HashSet::new(), image_dimensions: HashMap::new(), bytes_tx, bytes_rx, decode_tx, decode_rx }
    }

    pub fn poll(&mut self) -> ImagePipelinePoll {
        let mut changed = ImagePipelinePoll::default();
        while let Ok(msg) = self.bytes_rx.try_recv() {
            match msg {
                BytesResponse::Loaded(idx, bytes) => {
                    self.pending_bytes.remove(&idx);
                    self.bytes_cache.insert(idx, bytes.clone());
                    if !self.pending_decodes.contains(&idx) {
                        let _ = self.decode_tx.send(DecodeRequest::Decode(idx, bytes));
                        self.pending_decodes.insert(idx);
                    }
                }
                BytesResponse::Failed(idx) => {
                    self.pending_bytes.remove(&idx);
                }
            }
        }

        while let Ok(msg) = self.decode_rx.try_recv() {
            match msg {
                DecodeResponse::Decoded(idx, decoded) => {
                    self.pending_decodes.remove(&idx);
                    self.decoded_cache.insert(idx, decoded);
                    changed.decoded = true;
                    if let Some(image) = self.decoded_cache.get(&idx) {
                        let dimensions = (image.image.width as u32, image.image.height as u32);
                        let previous = self.image_dimensions.insert(idx, dimensions);
                        if previous != Some(dimensions) {
                            changed.dimensions_changed.push(idx);
                        }
                    }
                }
                DecodeResponse::Failed(idx) => {
                    self.pending_decodes.remove(&idx);
                }
            }
        }

        changed.dimensions_changed.sort_unstable();
        changed.dimensions_changed.dedup();
        changed
    }

    pub fn ensure_window(&mut self, desired: &HashSet<u32>) {
        self.bytes_cache.retain(|idx, _| desired.contains(idx));
        self.decoded_cache.retain(|idx, _| desired.contains(idx));
        self.pending_bytes.retain(|idx| desired.contains(idx));
        self.pending_decodes.retain(|idx| desired.contains(idx));

        for &idx in desired {
            if self.decoded_cache.contains_key(&idx) {
                continue;
            }
            if let Some(bytes) = self.bytes_cache.get(&idx) {
                if !self.pending_decodes.contains(&idx) {
                    let _ = self.decode_tx.send(DecodeRequest::Decode(idx, bytes.clone()));
                    self.pending_decodes.insert(idx);
                }
                continue;
            }
            if !self.pending_bytes.contains(&idx) {
                let _ = self.bytes_tx.send(BytesRequest::Load(idx));
                self.pending_bytes.insert(idx);
            }
        }
    }

    pub fn get_decoded(&self, idx: u32) -> Option<&DecodedImage> {
        self.decoded_cache.get(&idx)
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_bytes.is_empty() || !self.pending_decodes.is_empty()
    }
}

fn bytes_worker(images: Arc<Vec<ImageResource>>, provider: Arc<dyn ResourceProvider>, rx: mpsc::Receiver<BytesRequest>, tx: mpsc::Sender<BytesResponse>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            BytesRequest::Load(idx) => {
                let Some(image) = images.get(idx as usize) else {
                    let _ = tx.send(BytesResponse::Failed(idx));
                    continue;
                };
                match load_bytes(provider.as_ref(), &image.source) {
                    Ok(bytes) => {
                        let _ = tx.send(BytesResponse::Loaded(idx, Arc::new(bytes)));
                    }
                    Err(_) => {
                        let _ = tx.send(BytesResponse::Failed(idx));
                    }
                }
            }
        }
    }
}

fn decode_worker(rx: mpsc::Receiver<DecodeRequest>, tx: mpsc::Sender<DecodeResponse>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            DecodeRequest::Decode(idx, bytes) => match decode_image(&bytes) {
                Ok(decoded) => {
                    let _ = tx.send(DecodeResponse::Decoded(idx, decoded));
                }
                Err(_) => {
                    let _ = tx.send(DecodeResponse::Failed(idx));
                }
            },
        }
    }
}

fn decode_image(bytes: &[u8]) -> Result<DecodedImage, image::ImageError> {
    let image = image::load_from_memory(bytes)?;
    let width = image.width();
    let height = image.height();
    let rgba = image.into_rgba8().into_vec();
    let byte_len = rgba.len();
    let blob = Blob::new(Arc::new(rgba));
    let peniko = PenikoImage::new(blob, Format::Rgba8, width, height);

    let mut hasher = Sha256::new();
    hasher.update(peniko.data.data());
    let hash = hasher.finalize().to_vec();

    Ok(DecodedImage { image: peniko, hash, byte_len })
}

pub fn probe_dimensions(provider: &dyn ResourceProvider, source: &ImageSource) -> Option<(u32, u32)> {
    let bytes = load_bytes(provider, source).ok()?;
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.into_dimensions().ok()
}

fn load_bytes(provider: &dyn ResourceProvider, source: &ImageSource) -> std::io::Result<Vec<u8>> {
    match source {
        ImageSource::Uri(uri) => provider.read_bytes(uri),
    }
}
