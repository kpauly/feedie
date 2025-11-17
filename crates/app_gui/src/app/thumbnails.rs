use super::{MAX_FULL_IMAGES, MAX_THUMBS, THUMB_SIZE, UiApp};
use eframe::egui;
use std::path::Path;

impl UiApp {
    /// Loads a thumbnail texture for the requested path or reuses a cached entry.
    pub(super) fn get_or_load_thumb(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Option<egui::TextureId> {
        if let Some(tex) = self.thumbs.get(path) {
            return Some(tex.id());
        }

        match image::open(path) {
            Ok(img) => {
                // Ensure a 4-channel buffer for egui
                let rgba = img.to_rgba8();
                let thumb = image::imageops::thumbnail(&rgba, THUMB_SIZE, THUMB_SIZE);
                let (w, h) = thumb.dimensions();
                let size = [w as usize, h as usize];
                let pixels = thumb.into_raw(); // RGBA, len = w*h*4
                let color = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let name = format!("thumb:{}", path.display());
                let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                self.thumbs.insert(path.to_path_buf(), tex);
                self.thumb_keys.push_back(path.to_path_buf());
                if self.thumbs.len() > MAX_THUMBS
                    && let Some(old) = self.thumb_keys.pop_front()
                {
                    self.thumbs.remove(&old);
                }
                self.thumbs.get(path).map(|t| t.id())
            }
            Err(e) => {
                tracing::warn!("Failed to load thumbnail for {}: {}", path.display(), e);
                None
            }
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
