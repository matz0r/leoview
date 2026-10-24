//! Background image decoding.
//!
//! Thumbnails and full-size images are decoded on worker threads so the UI never blocks.
//! Each queue has a generation counter: bumping it makes workers skip all jobs queued
//! before, which keeps the queue from wasting time on images that are no longer wanted
//! (e.g. after switching directories).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui::{self, ColorImage};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};

/// Thumbnails are decoded at this maximum height (largest slider value).
pub const THUMB_MAX: u32 = 500;
/// Maximum thumbnail width, relative to `THUMB_MAX`, for panoramas.
const THUMB_MAX_ASPECT: u32 = 3;
/// Full images larger than this are downscaled to stay within GPU texture limits.
const FULL_MAX: u32 = 8192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Thumb,
    Full,
}

struct Job {
    path: PathBuf,
    generation: u64,
}

pub struct Loaded {
    pub path: PathBuf,
    pub kind: Kind,
    pub generation: u64,
    pub image: Result<ColorImage, String>,
}

struct Queue {
    tx: Sender<Job>,
    generation: Arc<AtomicU64>,
}

impl Queue {
    fn new(kind: Kind, workers: usize, results: &Sender<Loaded>, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let generation = Arc::new(AtomicU64::new(0));

        for _ in 0..workers {
            let rx = Arc::clone(&rx);
            let generation = Arc::clone(&generation);
            let results = results.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                loop {
                    let job = {
                        let rx = rx.lock().unwrap();
                        match rx.recv() {
                            Ok(job) => job,
                            Err(_) => break,
                        }
                    };
                    if job.generation != generation.load(Ordering::Relaxed) {
                        continue;
                    }
                    let image = decode(&job.path, kind);
                    let loaded = Loaded {
                        path: job.path,
                        kind,
                        generation: job.generation,
                        image,
                    };
                    if results.send(loaded).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }

        Self { tx, generation }
    }
}

/// Aspect ratio of image `index` in the list passed to [`Loader::scan_aspects`].
pub struct Aspect {
    pub generation: u64,
    pub index: usize,
    /// `None` if the header could not be read.
    pub aspect: Option<f32>,
}

pub struct Loader {
    thumbs: Queue,
    full: Queue,
    results: Receiver<Loaded>,
    aspect_tx: Sender<Aspect>,
    aspects: Receiver<Aspect>,
    aspect_generation: Arc<AtomicU64>,
    ctx: egui::Context,
}

impl Loader {
    pub fn new(ctx: &egui::Context) -> Self {
        let (results_tx, results) = mpsc::channel();
        let (aspect_tx, aspects) = mpsc::channel();
        let thumb_workers = thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).clamp(1, 8))
            .unwrap_or(2);
        Self {
            thumbs: Queue::new(Kind::Thumb, thumb_workers, &results_tx, ctx),
            full: Queue::new(Kind::Full, 1, &results_tx, ctx),
            results,
            aspect_tx,
            aspects,
            aspect_generation: Arc::new(AtomicU64::new(0)),
            ctx: ctx.clone(),
        }
    }

    /// Reads the aspect ratios of `paths` on a background thread, cancelling any
    /// previous scan. Results arrive through [`Self::try_recv_aspect`].
    pub fn scan_aspects(&self, paths: Vec<PathBuf>) {
        let generation = self.aspect_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let current = Arc::clone(&self.aspect_generation);
        let tx = self.aspect_tx.clone();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            for (index, path) in paths.iter().enumerate() {
                if current.load(Ordering::Relaxed) != generation {
                    return;
                }
                let aspect = read_aspect(path);
                if tx
                    .send(Aspect {
                        generation,
                        index,
                        aspect,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    pub fn aspect_generation(&self) -> u64 {
        self.aspect_generation.load(Ordering::Relaxed)
    }

    pub fn try_recv_aspect(&self) -> Option<Aspect> {
        self.aspects.try_recv().ok()
    }

    fn queue(&self, kind: Kind) -> &Queue {
        match kind {
            Kind::Thumb => &self.thumbs,
            Kind::Full => &self.full,
        }
    }

    /// Queue an image for decoding in the current generation.
    pub fn request(&self, kind: Kind, path: PathBuf) {
        let queue = self.queue(kind);
        let generation = queue.generation.load(Ordering::Relaxed);
        // Workers only stop when the loader is dropped, so a send error can be ignored.
        let _ = queue.tx.send(Job { path, generation });
    }

    /// Invalidate all queued jobs of the given kind.
    pub fn new_generation(&self, kind: Kind) {
        self.queue(kind).generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn generation(&self, kind: Kind) -> u64 {
        self.queue(kind).generation.load(Ordering::Relaxed)
    }

    pub fn try_recv(&self) -> Option<Loaded> {
        self.results.try_recv().ok()
    }
}

fn decode(path: &Path, kind: Kind) -> Result<ColorImage, String> {
    let mut decoder = ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;

    let (max_w, max_h) = match kind {
        Kind::Thumb => (THUMB_MAX * THUMB_MAX_ASPECT, THUMB_MAX),
        Kind::Full => (FULL_MAX, FULL_MAX),
    };
    // Orientation is applied afterwards, so swap the bounds for rotated images.
    let (max_w, max_h) = if swaps_axes(orientation) {
        (max_h, max_w)
    } else {
        (max_w, max_h)
    };
    if img.width() > max_w || img.height() > max_h {
        img = img.thumbnail(max_w, max_h);
    }
    img.apply_orientation(orientation);

    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// Reads the displayed aspect ratio (width / height) from the file header only.
fn read_aspect(path: &Path) -> Option<f32> {
    let mut decoder = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (w, h) = if swaps_axes(orientation) {
        (h, w)
    } else {
        (w, h)
    };
    Some(w as f32 / h as f32)
}

fn swaps_axes(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}
