//! # feeder_core
//!
//! `feeder_core` exposes the building blocks for scanning folders, running the
//! EfficientViT classifier, and exporting CSV data. This crate is kept UI-free
//! so both the GUI and any future CLI or service can reuse the same inference
//! pipeline.
//!
//! ## Examples
//!
//! ```no_run
//! use feeder_core::{scan_folder, EfficientVitClassifier, ClassifierConfig};
//!
//! # fn run() -> anyhow::Result<()> {
//! let rows = scan_folder("/path/to/images")?;
//! let config = ClassifierConfig::default();
//! let mut classifier = EfficientVitClassifier::new(&config)?;
//! classifier.classify_with_progress(&mut rows.clone(), |done, total| {
//!     println!("{done}/{total}");
//! })?;
//! # Ok(())
//! # }
//! # run().unwrap();
//! ```

use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use fast_image_resize::{self as fr, images::Image as FrImage};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub use classifier::{ClassifierConfig, EfficientVitClassifier, EfficientVitVariant};

/// Classification decision for an image/crop.
///
/// This is used throughout the pipeline to describe the classifier outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Unknown,
    Label(String),
}

/// Classification result with decision and confidence.
///
/// Instances appear in [`ImageInfo::classification`] when a decision was made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Classification {
    pub decision: Decision,
    pub confidence: f32,
}

/// Core image information gathered by the pipeline.
///
/// The GUI consumes this type directly to drive previews and exports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageInfo {
    /// Absolute path to the file on disk.
    pub file: PathBuf,
    /// Whether the classifier believes a species is present.
    pub present: bool,
    /// Optional classifier output with decision and confidence.
    pub classification: Option<Classification>,
}

/// Options controlling how folder scanning behaves.
///
/// `scan_folder_with` reads these to decide whether to recurse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    /// When true, scan subdirectories recursively.
    pub recursive: bool,
}

/// Scan a folder for images and produce basic `ImageInfo` entries.
///
/// This is a convenience wrapper around [`scan_folder_with`] using default
/// options. It filters files by extension and does not recurse by default.
///
/// # Errors
///
/// Returns an error if the path does not exist or is not a directory.
///
/// # Examples
///
/// ```no_run
/// let infos = feeder_core::scan_folder("/data/camera")?;
/// println!("{} frames discovered", infos.len());
/// # Ok::<_, anyhow::Error>(())
/// ```
pub fn scan_folder(path: impl AsRef<Path>) -> Result<Vec<ImageInfo>> {
    scan_folder_with(path, ScanOptions::default())
}

/// Scan a folder with options.
///
/// # Errors
///
/// Returns an error when the path is missing or not a directory.
///
/// # Examples
///
/// ```no_run
/// use feeder_core::{scan_folder_with, ScanOptions};
/// let infos = scan_folder_with("/data/camera", ScanOptions { recursive: true })?;
/// # Ok::<_, anyhow::Error>(())
/// ```
pub fn scan_folder_with(path: impl AsRef<Path>, opts: ScanOptions) -> Result<Vec<ImageInfo>> {
    let root = path.as_ref();
    if !root.exists() {
        anyhow::bail!("Pad bestaat niet: {}", root.display());
    }
    if !root.is_dir() {
        anyhow::bail!("Pad is geen map: {}", root.display());
    }

    let mut infos: Vec<ImageInfo> = Vec::new();
    let walker = if opts.recursive {
        WalkDir::new(root).into_iter()
    } else {
        WalkDir::new(root).max_depth(1).into_iter()
    };

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("walkdir fout: {}", e);
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if is_supported_image(path) {
            infos.push(ImageInfo {
                file: path.to_path_buf(),
                present: false,
                classification: None,
            });
        }
    }

    Ok(infos)
}

/// Export the provided rows to CSV with headers `file,present,species,confidence`.
///
/// # Errors
///
/// Returns any I/O or serialization errors encountered while writing the CSV.
///
/// # Examples
///
/// ```no_run
/// # use feeder_core::{ImageInfo, export_csv, Decision, Classification};
/// # use std::path::PathBuf;
/// let rows = vec![ImageInfo {
///     file: PathBuf::from("/tmp/frame.jpg"),
///     present: true,
///     classification: Some(Classification {
///         decision: Decision::Label("koolmees".into()),
///         confidence: 0.92,
///     }),
/// }];
/// export_csv(&rows, "/tmp/results.csv")?;
/// # Ok::<_, anyhow::Error>(())
/// ```
pub fn export_csv(rows: &[ImageInfo], path: impl AsRef<Path>) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(["file", "present", "species", "confidence"])?;

    for info in rows {
        let (species, confidence): (Option<String>, Option<f32>) = if info.present {
            match &info.classification {
                Some(Classification {
                    decision,
                    confidence,
                }) => {
                    let s = match decision {
                        Decision::Unknown => Some("Unknown".to_string()),
                        Decision::Label(name) => Some(name.clone()),
                    };
                    (s, Some(*confidence))
                }
                None => (None, None),
            }
        } else {
            (None, None)
        };

        let species_field = species.unwrap_or_default();
        let confidence_field = confidence
            .map(|c| format!("{c}"))
            .unwrap_or_else(String::new);

        wtr.write_record([
            info.file.to_string_lossy().as_ref(),
            if info.present { "true" } else { "false" },
            species_field.as_str(),
            confidence_field.as_str(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

/// Returns true when the file extension is supported by the classifier.
fn is_supported_image(path: &Path) -> bool {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            matches!(ext.as_str(), "jpg" | "jpeg" | "png")
        }
        None => false,
    }
}

/// Resizes an image to a fixed square using a SIMD-aware resizer.
fn resize_to_square_rgb(img: DynamicImage, size: u32) -> Result<Vec<u8>> {
    let rgb = img.into_rgb8();
    let (width, height) = (rgb.width(), rgb.height());
    let src = FrImage::from_vec_u8(width, height, rgb.into_raw(), fr::PixelType::U8x3)
        .context("resize source buffer invalid")?;
    let mut dst = FrImage::new(size, size, fr::PixelType::U8x3);
    let mut resizer = fr::Resizer::new();
    let options = fr::ResizeOptions {
        algorithm: fr::ResizeAlg::Convolution(fr::FilterType::Bilinear),
        ..Default::default()
    };
    resizer
        .resize(&src, &mut dst, Some(&options))
        .context("resize failed")?;
    Ok(dst.into_vec())
}

/// Convert an image file into a normalized tensor (CHW) on the provided device.
///
/// # Errors
///
/// Returns an error when the file cannot be decoded or tensor creation fails.
///
/// # Examples
///
/// ```no_run
/// # use candle_core::Device;
/// let tensor = feeder_core::load_image_tensor(
///     std::path::Path::new("/tmp/frame.jpg"),
///     224,
///     [0.485, 0.456, 0.406],
///     [0.229, 0.224, 0.225],
///     &Device::Cpu,
/// )?;
/// assert_eq!(tensor.dims(), &[3, 224, 224]);
/// # Ok::<_, anyhow::Error>(())
/// ```
pub fn load_image_tensor(
    path: &Path,
    size: u32,
    mean: [f32; 3],
    std: [f32; 3],
    device: &Device,
) -> Result<Tensor> {
    let data = load_image_tensor_data(path, size, mean, std)?;
    let tensor = Tensor::from_vec(data, (3, size as usize, size as usize), device)?;
    Ok(tensor)
}

/// Internal helper that loads the pixel data and normalizes channels.
fn load_image_tensor_data(
    path: &Path,
    size: u32,
    mean: [f32; 3],
    std: [f32; 3],
) -> Result<Vec<f32>> {
    let img = image::open(path)?;
    let resized = resize_to_square_rgb(img, size)?;
    let hw = (size * size) as usize;
    let mut data = vec![0f32; hw * 3];
    for idx in 0..hw {
        let base = idx * 3;
        data[idx] = normalize_channel(resized[base], mean[0], std[0]);
        data[hw + idx] = normalize_channel(resized[base + 1], mean[1], std[1]);
        data[2 * hw + idx] = normalize_channel(resized[base + 2], mean[2], std[2]);
    }
    Ok(data)
}

/// Normalizes a single channel given ImageNet mean/std parameters.
fn normalize_channel(value: u8, mean: f32, std: f32) -> f32 {
    let v = value as f32 / 255.0;
    (v - mean) / std
}

/// EfficientViT classifier implementation and configuration helpers.
mod classifier {
    use super::{Classification, Decision, ImageInfo, load_image_tensor_data};
    use anyhow::{Context, Result};
    use candle_core::{D, DType, Device, Tensor};
    use candle_nn::{self as nn, Func, Module, VarBuilder};
    use candle_transformers::models::efficientvit::{
        self as efficientvit_model, Config as EfficientVitConfig,
    };
    use rayon::prelude::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock, mpsc};
    use std::thread;
    use std::time::Instant;

    struct TimingLogger {
        file: Mutex<std::fs::File>,
    }

    impl TimingLogger {
        fn new(path: PathBuf) -> Option<Self> {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            Some(Self {
                file: Mutex::new(file),
            })
        }

        fn log(&self, line: &str) {
            if let Ok(mut file) = self.file.lock() {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    fn timing_logger() -> Option<&'static TimingLogger> {
        static LOGGER: OnceLock<Option<TimingLogger>> = OnceLock::new();
        LOGGER
            .get_or_init(|| {
                let path = std::env::var("FEEDER_TIMING_LOG").ok()?;
                TimingLogger::new(PathBuf::from(path))
            })
            .as_ref()
    }

    const PIPELINE_QUEUE_DEPTH: usize = 2;

    struct BatchSpec {
        start: usize,
        files: Vec<PathBuf>,
    }

    struct PreparedBatch {
        start: usize,
        len: usize,
        items: Vec<(usize, PathBuf, Result<Vec<f32>>)>,
        prep_ms: u128,
    }

    /// Enumerates the EfficientViT variants this crate knows about.
    #[derive(Debug, Clone, Copy, Default)]
    pub enum EfficientVitVariant {
        #[default]
        M0,
        M1,
        M2,
        M3,
        M4,
        M5,
    }

    impl EfficientVitVariant {
        /// Returns the canonical transformer configuration for this variant.
        pub fn config(&self) -> EfficientVitConfig {
            match self {
                Self::M0 => EfficientVitConfig::m0(),
                Self::M1 => EfficientVitConfig::m1(),
                Self::M2 => EfficientVitConfig::m2(),
                Self::M3 => EfficientVitConfig::m3(),
                Self::M4 => EfficientVitConfig::m4(),
                Self::M5 => EfficientVitConfig::m5(),
            }
        }
    }

    /// Configuration for the Candle-based EfficientViT classifier.
    #[derive(Debug, Clone)]
    /// Configuration used to build an [`EfficientVitClassifier`].
    ///
    /// Values default to the bundled EfficientViT-M0 settings but can be tweaked
    /// when running custom models or different batching strategies.
    pub struct ClassifierConfig {
        /// Path to the `.safetensors` model to load.
        pub model_path: PathBuf,
        /// Path to the CSV file that lists labels in order.
        pub labels_path: PathBuf,
        /// EfficientViT variant describing the architecture; affects the config.
        pub variant: EfficientVitVariant,
        /// Width/height of the resized square input.
        pub input_size: u32,
        /// Threshold above which detections count as “present”.
        pub presence_threshold: f32,
        /// Mean normalization per channel (RGB order).
        pub mean: [f32; 3],
        /// Std deviation normalization per channel (RGB order).
        pub std: [f32; 3],
        /// Canonical labels that should be treated as background.
        pub background_labels: Vec<String>,
        /// Number of images to classify per batch.
        pub batch_size: usize,
    }

    impl Default for ClassifierConfig {
        fn default() -> Self {
            Self {
                model_path: PathBuf::from("models/feeder-efficientvit-m0.safetensors"),
                labels_path: PathBuf::from("models/feeder-labels.csv"),
                variant: EfficientVitVariant::M0,
                input_size: 224,
                presence_threshold: 0.5,
                mean: [0.485, 0.456, 0.406],
                std: [0.229, 0.224, 0.225],
                background_labels: vec!["Achtergrond".to_string()],
                batch_size: 8,
            }
        }
    }

    /// High-level wrapper around the EfficientViT model used to classify images.
    ///
    /// This struct owns the loaded model, label list, and normalization values.
    /// Call [`EfficientVitClassifier::classify_with_progress`] to mutate
    /// [`ImageInfo`] entries with predictions.
    pub struct EfficientVitClassifier {
        model: Func<'static>,
        device: Device,
        labels: Vec<String>,
        input_size: u32,
        presence_threshold: f32,
        mean: [f32; 3],
        std: [f32; 3],
        background_labels: Vec<String>,
        batch_size: usize,
    }

    impl EfficientVitClassifier {
        /// Loads the model weights, labels, and normalization settings.
        ///
        /// # Errors
        ///
        /// Returns an error when the model or label files are missing or cannot
        /// be parsed, or when the underlying tensors fail to load on the device.
        pub fn new(cfg: &ClassifierConfig) -> Result<Self> {
            if !cfg.model_path.exists() {
                anyhow::bail!(
                    "Modelbestand ontbreekt: {}",
                    cfg.model_path.to_string_lossy()
                );
            }
            if !cfg.labels_path.exists() {
                anyhow::bail!(
                    "Labels-bestand ontbreekt: {}",
                    cfg.labels_path.to_string_lossy()
                );
            }

            let labels_raw =
                fs::read_to_string(&cfg.labels_path).context("labels niet te lezen")?;
            let mut labels: Vec<String> = labels_raw
                .lines()
                .map(|line| {
                    let trimmed = line.trim();
                    let primary = trimmed
                        .split_once(',')
                        .map(|(first, _)| first.trim())
                        .unwrap_or(trimmed)
                        .trim_end_matches(',')
                        .trim();
                    primary.to_string()
                })
                .filter(|l| !l.is_empty())
                .collect();
            if labels.is_empty() {
                anyhow::bail!("labels-bestand bevat geen labels");
            }
            labels.dedup();

            let device = Device::Cpu;
            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(
                    std::slice::from_ref(&cfg.model_path),
                    DType::F32,
                    &device,
                )?
            };
            let vit_config = cfg.variant.config();
            let model = efficientvit_model::efficientvit(&vit_config, labels.len(), vb)?;

            Ok(Self {
                model,
                device,
                labels,
                input_size: cfg.input_size,
                presence_threshold: cfg.presence_threshold,
                mean: cfg.mean,
                std: cfg.std,
                background_labels: cfg
                    .background_labels
                    .iter()
                    .map(|s| s.to_ascii_lowercase())
                    .collect(),
                batch_size: cfg.batch_size.max(1),
            })
        }

        /// Classifies the provided rows in batches and reports progress.
        ///
        /// `rows` are updated in-place based on the classifier output. The
        /// callback receives `(done, total)` after each processed batch.
        ///
        /// # Errors
        ///
        /// Returns an error if tensor creation or model evaluation fails.
        pub fn classify_with_progress<F>(&self, rows: &mut [ImageInfo], progress: F) -> Result<()>
        where
            F: FnMut(usize, usize),
        {
            self.classify_with_progress_and_batch_size(rows, self.batch_size, progress)
        }

        /// Classifies the provided rows using the supplied batch size.
        pub fn classify_with_progress_and_batch_size<F>(
            &self,
            rows: &mut [ImageInfo],
            batch_size: usize,
            mut progress: F,
        ) -> Result<()>
        where
            F: FnMut(usize, usize),
        {
            let total = rows.len();
            if total == 0 {
                return Ok(());
            }

            let mut processed = 0usize;
            let batch_size = batch_size.max(1);
            let specs: Vec<BatchSpec> = rows
                .chunks(batch_size)
                .enumerate()
                .map(|(batch_idx, chunk)| BatchSpec {
                    start: batch_idx * batch_size,
                    files: chunk.iter().map(|info| info.file.clone()).collect(),
                })
                .collect();
            let wants_timing = timing_logger().is_some();
            let (tx, rx) = mpsc::sync_channel(PIPELINE_QUEUE_DEPTH);
            let input_size = self.input_size;
            let mean = self.mean;
            let std = self.std;
            thread::spawn(move || {
                for spec in specs {
                    let prepared = Self::prepare_batch(spec, input_size, mean, std, wants_timing);
                    if tx.send(prepared).is_err() {
                        break;
                    }
                }
            });

            let logger = timing_logger();
            for prepared in rx {
                let start = prepared.start;
                let len = prepared.len;
                if len == 0 {
                    continue;
                }
                let chunk = &mut rows[start..start + len];
                let mut tensor_order: Vec<usize> = Vec::new();
                let mut tensors: Vec<Tensor> = Vec::new();
                for (idx, path, data_res) in prepared.items {
                    match data_res {
                        Ok(data) => match self.tensor_from_data(data) {
                            Ok(tensor) => {
                                tensor_order.push(idx);
                                tensors.push(tensor);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Tensor bouwen mislukt voor {}: {err}",
                                    path.display()
                                );
                                if let Some(info) = chunk.get_mut(idx) {
                                    info.present = false;
                                    info.classification = None;
                                }
                            }
                        },
                        Err(err) => {
                            tracing::warn!(
                                "Afbeelding laden mislukt voor {}: {err}",
                                path.display()
                            );
                            if let Some(info) = chunk.get_mut(idx) {
                                info.present = false;
                                info.classification = None;
                            }
                        }
                    }
                }

                if tensors.is_empty() {
                    if let Some(logger) = logger {
                        let prep_ms = prepared.prep_ms;
                        logger.log(&format!(
                            "batch_size={}, chunk_len={}, tensors=0, prep_ms={}, forward_ms=0, total_ms={}",
                            batch_size, len, prep_ms, prep_ms
                        ));
                    }
                    processed += len;
                    progress(processed.min(total), total);
                    continue;
                }

                let forward_start = logger.map(|_| Instant::now());
                let views = tensors.iter().collect::<Vec<_>>();
                let batch = Tensor::stack(&views, 0)?;
                let logits = self.model.forward(&batch)?;
                let probs = nn::ops::softmax(&logits, D::Minus1)?;
                let probs_rows = probs.to_vec2::<f32>()?;
                let forward_ms = forward_start.map(|start| start.elapsed().as_millis());

                if let Some(logger) = logger {
                    let prep_ms = prepared.prep_ms;
                    let forward_ms = forward_ms.unwrap_or(0);
                    let total_ms = prep_ms + forward_ms;
                    logger.log(&format!(
                        "batch_size={}, chunk_len={}, tensors={}, prep_ms={}, forward_ms={}, total_ms={}",
                        batch_size,
                        len,
                        tensors.len(),
                        prep_ms,
                        forward_ms,
                        total_ms
                    ));
                }

                for (row_probs, idx_in_chunk) in
                    probs_rows.into_iter().zip(tensor_order.into_iter())
                {
                    if let Some(info) = chunk.get_mut(idx_in_chunk) {
                        match self.build_result_from_probs(&row_probs) {
                            Ok(result) => {
                                info.present = result.present;
                                info.classification = result.classification;
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Resultaat opbouwen mislukt voor {}: {err}",
                                    info.file.display()
                                );
                                info.present = false;
                                info.classification = None;
                            }
                        }
                    }
                }

                processed += len;
                progress(processed.min(total), total);
            }

            Ok(())
        }

        fn prepare_batch(
            spec: BatchSpec,
            input_size: u32,
            mean: [f32; 3],
            std: [f32; 3],
            wants_timing: bool,
        ) -> PreparedBatch {
            let prep_start = wants_timing.then(Instant::now);
            let len = spec.files.len();
            let mut prepared: Vec<_> = spec
                .files
                .into_par_iter()
                .enumerate()
                .map(|(idx, path)| {
                    let data = load_image_tensor_data(&path, input_size, mean, std);
                    (idx, path, data)
                })
                .collect();
            prepared.sort_by_key(|(idx, _, _)| *idx);
            let prep_ms = prep_start
                .map(|start| start.elapsed().as_millis())
                .unwrap_or(0);
            PreparedBatch {
                start: spec.start,
                len,
                items: prepared,
                prep_ms,
            }
        }

        fn tensor_from_data(&self, data: Vec<f32>) -> Result<Tensor> {
            Ok(Tensor::from_vec(
                data,
                (3, self.input_size as usize, self.input_size as usize),
                &self.device,
            )?)
        }

        fn build_result_from_probs(&self, probs: &[f32]) -> Result<ClassificationResult> {
            if probs.is_empty() {
                anyhow::bail!("lege logits");
            }
            let (best_idx, &best_prob) = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            let label = self
                .labels
                .get(best_idx)
                .cloned()
                .unwrap_or_else(|| format!("class_{best_idx}"));
            let label_lower = label.to_ascii_lowercase();
            let is_background = self.background_labels.iter().any(|bg| bg == &label_lower);
            let present = best_prob >= self.presence_threshold && !is_background;
            let decision = if is_background {
                Decision::Unknown
            } else {
                Decision::Label(label)
            };
            Ok(ClassificationResult {
                present,
                classification: Some(Classification {
                    decision,
                    confidence: best_prob,
                }),
            })
        }
    }

    struct ClassificationResult {
        present: bool,
        classification: Option<Classification>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn export_csv_writes_expected_headers_and_rows() -> Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("out.csv");
        let rows = vec![
            ImageInfo {
                file: PathBuf::from("a.jpg"),
                present: false,
                classification: None,
            },
            ImageInfo {
                file: PathBuf::from("b.jpg"),
                present: true,
                classification: Some(Classification {
                    decision: Decision::Unknown,
                    confidence: 0.42,
                }),
            },
            ImageInfo {
                file: PathBuf::from("c.jpg"),
                present: true,
                classification: Some(Classification {
                    decision: Decision::Label("Sparrow".into()),
                    confidence: 0.91,
                }),
            },
        ];

        export_csv(&rows, &path)?;

        let mut rdr = csv::Reader::from_path(&path)?;
        let headers = rdr.headers()?.clone();
        assert_eq!(
            headers.iter().collect::<Vec<_>>(),
            vec!["file", "present", "species", "confidence"]
        );

        let mut recs = rdr.records();
        let r1 = recs.next().unwrap()?;
        assert_eq!(&r1[0], "a.jpg");
        assert_eq!(&r1[1], "false");
        assert_eq!(&r1[2], "");
        assert_eq!(&r1[3], "");

        let r2 = recs.next().unwrap()?;
        assert_eq!(&r2[0], "b.jpg");
        assert_eq!(&r2[1], "true");
        assert_eq!(&r2[2], "Unknown");
        assert_eq!(&r2[3], "0.42");

        let r3 = recs.next().unwrap()?;
        assert_eq!(&r3[0], "c.jpg");
        assert_eq!(&r3[1], "true");
        assert_eq!(&r3[2], "Sparrow");
        assert_eq!(&r3[3], "0.91");

        assert!(recs.next().is_none());
        Ok(())
    }

    #[test]
    fn scan_folder_empty_returns_empty() -> Result<()> {
        let dir = tempdir()?;
        let rows = scan_folder(dir.path())?;
        assert!(rows.is_empty());
        Ok(())
    }

    #[test]
    fn scan_folder_lists_only_images_non_recursive() -> Result<()> {
        let dir = tempdir()?;
        File::create(dir.path().join("a.JPG"))?;
        File::create(dir.path().join("b.jpeg"))?;
        File::create(dir.path().join("c.png"))?;
        File::create(dir.path().join("not-image.txt"))?;
        let nested = dir.path().join("nested");
        fs::create_dir(&nested)?;
        File::create(nested.join("d.jpg"))?;

        let rows = scan_folder_with(dir.path(), ScanOptions { recursive: false })?;
        let mut files: Vec<String> = rows
            .into_iter()
            .map(|i| i.file.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        files.sort();
        assert_eq!(files, vec!["a.JPG", "b.jpeg", "c.png"]);
        Ok(())
    }

    #[test]
    fn scan_folder_lists_images_recursive_when_enabled() -> Result<()> {
        let dir = tempdir()?;
        File::create(dir.path().join("a.jpg"))?;
        let nested = dir.path().join("nested");
        fs::create_dir(&nested)?;
        File::create(nested.join("b.PNG"))?;

        let rows = scan_folder_with(dir.path(), ScanOptions { recursive: true })?;
        let mut files: Vec<String> = rows
            .into_iter()
            .map(|i| i.file.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        files.sort();
        assert_eq!(files, vec!["a.jpg", "b.PNG"]);
        Ok(())
    }
}
