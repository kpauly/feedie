//! Thumbnail and preview texture caching helpers.

use super::{
    MAX_FULL_IMAGES, MAX_THUMB_APPLY_PER_FRAME, MAX_THUMBS, THUMB_SIZE, ThumbRequest, ThumbResult,
    UiApp,
};
use eframe::egui;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

pub(super) fn spawn_thumbnail_worker() -> (Sender<ThumbRequest>, Receiver<ThumbResult>) {
    let (req_tx, req_rx) = mpsc::channel::<ThumbRequest>();
    let (res_tx, res_rx) = mpsc::channel::<ThumbResult>();

    thread::spawn(move || {
        for request in req_rx {
            let result = match image::open(&request.path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let thumb = image::imageops::thumbnail(&rgba, THUMB_SIZE, THUMB_SIZE);
                    let (w, h) = thumb.dimensions();
                    ThumbResult {
                        path: request.path,
                        generation: request.generation,
                        size: [w as usize, h as usize],
                        pixels: thumb.into_raw(),
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to load thumbnail for {}: {}",
                        request.path.display(),
                        err
                    );
                    ThumbResult {
                        path: request.path,
                        generation: request.generation,
                        size: [0, 0],
                        pixels: Vec::new(),
                    }
                }
            };
            let _ = res_tx.send(result);
        }
    });

    (req_tx, res_rx)
}

impl UiApp {
    pub(crate) fn reset_thumbnail_cache(&mut self) {
        self.thumbs.clear();
        self.thumb_keys.clear();
        self.thumb_inflight.clear();
        self.thumb_failed.clear();
        self.thumb_generation = self.thumb_generation.wrapping_add(1);
    }

    pub(crate) fn thumb_texture_id(&self, path: &Path) -> Option<egui::TextureId> {
        self.thumbs.get(path).map(|tex| tex.id())
    }

    pub(crate) fn queue_thumbnails_for_indices(&mut self, indices: &[usize]) {
        let paths: Vec<_> = indices
            .iter()
            .filter_map(|&idx| self.rijen.get(idx).map(|info| info.file.clone()))
            .collect();
        for path in paths {
            self.queue_thumbnail(&path);
        }
    }

    pub(crate) fn poll_thumbnail_results(&mut self, ctx: &egui::Context) {
        let mut processed = 0usize;
        while processed < MAX_THUMB_APPLY_PER_FRAME {
            match self.thumb_res_rx.try_recv() {
                Ok(result) => {
                    processed += 1;
                    self.thumb_inflight.remove(&result.path);
                    if result.generation != self.thumb_generation {
                        continue;
                    }
                    if result.pixels.is_empty() || result.size[0] == 0 || result.size[1] == 0 {
                        self.thumb_failed.insert(result.path);
                        continue;
                    }
                    if self.thumbs.contains_key(&result.path) {
                        continue;
                    }
                    let color =
                        egui::ColorImage::from_rgba_unmultiplied(result.size, &result.pixels);
                    let name = format!("thumb:{}", result.path.display());
                    let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                    self.thumbs.insert(result.path.clone(), tex);
                    self.thumb_keys.push_back(result.path);
                    if self.thumbs.len() > MAX_THUMBS
                        && let Some(old) = self.thumb_keys.pop_front()
                    {
                        self.thumbs.remove(&old);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn queue_thumbnail(&mut self, path: &Path) {
        if self.thumbs.contains_key(path)
            || self.thumb_inflight.contains(path)
            || self.thumb_failed.contains(path)
        {
            return;
        }
        self.thumb_inflight.insert(path.to_path_buf());
        let request = ThumbRequest {
            path: path.to_path_buf(),
            generation: self.thumb_generation,
        };
        if self.thumb_req_tx.send(request).is_err() {
            self.thumb_inflight.remove(path);
        }
    }

    /// Loads the full resolution texture that powers the preview window.
    pub(super) fn get_or_load_full_image(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<egui::TextureHandle> {
        if let Some(tex) = self.full_images.get(path) {
            return Some(tex.clone());
        }
        match image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let size = [w as usize, h as usize];
                let pixels = rgba.into_raw();
                let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let name = format!("full:{}", path.display());
                let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                self.full_images.insert(path.to_path_buf(), tex.clone());
                self.full_keys.push_back(path.to_path_buf());
                while self.full_images.len() > MAX_FULL_IMAGES {
                    if let Some(old) = self.full_keys.pop_front() {
                        self.full_images.remove(&old);
                    } else {
                        break;
                    }
                }
                Some(tex)
            }
            Err(e) => {
                tracing::warn!("Failed to load full image for {}: {}", path.display(), e);
                None
            }
        }
    }
}
