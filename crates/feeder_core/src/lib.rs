use anyhow::Result;
use image::{DynamicImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub use classifier::{ClassifierConfig, EfficientNetClassifier, EfficientNetVariant};
pub use training::{DatasetSample, DatasetSplit, TrainingConfig, load_dataset};

/// Classification decision for an image/crop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Unknown,
    Label(String),
}

/// Classification result with decision and confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Classification {
    pub decision: Decision,
    pub confidence: f32,
}

/// Core image information gathered by the pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub file: PathBuf,
    pub present: bool,
    pub classification: Option<Classification>,
}

/// Options controlling how folder scanning behaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    /// When true, scan subdirectories recursively.
    pub recursive: bool,
}

/// Scan a folder for images and produce basic `ImageInfo` entries.
pub fn scan_folder(path: impl AsRef<Path>) -> Result<Vec<ImageInfo>> {
    scan_folder_with(path, ScanOptions::default())
}

/// Scan a folder with options.
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

/// Export the provided rows to CSV with headers:
/// file,present,species,confidence
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

fn is_supported_image(path: &Path) -> bool {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            matches!(ext.as_str(), "jpg" | "jpeg" | "png")
        }
        None => false,
    }
}

fn resize_to_square(img: DynamicImage, size: u32) -> DynamicImage {
    img.resize_exact(size, size, FilterType::Triangle)
}

mod classifier {
    use super::{Classification, Decision, ImageInfo, resize_to_square};
    use anyhow::{Context, Result, anyhow};
    use candle_core::{D, DType, Device, Tensor};
    use candle_nn::{self as nn, Module, VarBuilder};
    use candle_transformers::models::efficientnet::{EfficientNet, MBConvConfig};
    use image::{self, GenericImageView};
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug, Clone, Copy, Default)]
    pub enum EfficientNetVariant {
        #[default]
        B0,
        B1,
        B2,
    }
    impl EfficientNetVariant {
        fn configs(&self) -> Vec<MBConvConfig> {
            match self {
                Self::B0 => MBConvConfig::b0(),
                Self::B1 => MBConvConfig::b1(),
                Self::B2 => MBConvConfig::b2(),
            }
        }
    }

    /// Configuration for the Candle-based EfficientNet classifier.
    #[derive(Debug, Clone)]
    pub struct ClassifierConfig {
        pub model_path: PathBuf,
        pub labels_path: PathBuf,
        pub variant: EfficientNetVariant,
        pub input_size: u32,
        pub presence_threshold: f32,
        pub mean: [f32; 3],
        pub std: [f32; 3],
        pub background_labels: Vec<String>,
    }

    impl Default for ClassifierConfig {
        fn default() -> Self {
            Self {
                model_path: PathBuf::from("models/efficientnet_b0.safetensors"),
                labels_path: PathBuf::from("models/labels.csv"),
                variant: EfficientNetVariant::B0,
                input_size: 512,
                presence_threshold: 0.5,
                mean: [0.485, 0.456, 0.406],
                std: [0.229, 0.224, 0.225],
                background_labels: vec!["Achtergrond".to_string()],
            }
        }
    }

    pub struct EfficientNetClassifier {
        model: EfficientNet,
        device: Device,
        labels: Vec<String>,
        input_size: u32,
        presence_threshold: f32,
        mean: [f32; 3],
        std: [f32; 3],
        background_labels: Vec<String>,
    }

    impl EfficientNetClassifier {
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
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
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
            let model = EfficientNet::new(vb, cfg.variant.configs(), labels.len())?;

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
            })
        }

        pub fn classify_with_progress<F>(
            &self,
            rows: &mut [ImageInfo],
            mut progress: F,
        ) -> Result<()>
        where
            F: FnMut(usize, usize),
        {
            let total = rows.len();
            if total == 0 {
                return Ok(());
            }
            for (idx, info) in rows.iter_mut().enumerate() {
                match self.classify_single(&info.file) {
                    Ok(result) => {
                        info.present = result.present;
                        info.classification = result.classification;
                    }
                    Err(err) => {
                        tracing::warn!("Classifier fout voor {}: {err}", info.file.display());
                        info.present = false;
                        info.classification = None;
                    }
                }
                progress(idx + 1, total);
            }
            Ok(())
        }

        fn classify_single(&self, path: &PathBuf) -> Result<ClassificationResult> {
            let tensor = self.prepare_input(path)?;
            let logits = self.model.forward(&tensor)?;
            let probs = nn::ops::softmax(&logits, D::Minus1)?.squeeze(0)?;
            let probs_vec = probs.to_vec1::<f32>()?;
            if probs_vec.is_empty() {
                anyhow::bail!("lege logits");
            }
            let (best_idx, &best_prob) = probs_vec
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            let label = self
                .labels
                .get(best_idx)
                .cloned()
                .unwrap_or_else(|| format!("class_{best_idx}"));
            let is_background = self
                .background_labels
                .iter()
                .any(|bg| bg == &label.to_ascii_lowercase());
            let present = best_prob >= self.presence_threshold && !is_background;
            let classification = if present {
                Some(Classification {
                    decision: Decision::Label(label),
                    confidence: best_prob,
                })
            } else {
                Some(Classification {
                    decision: Decision::Unknown,
                    confidence: best_prob,
                })
            };
            Ok(ClassificationResult {
                present,
                classification,
            })
        }

        fn prepare_input(&self, path: &PathBuf) -> Result<Tensor> {
            let img = image::open(path)
                .with_context(|| format!("kan afbeelding niet openen: {}", path.display()))?;
            let resized = resize_to_square(img, self.input_size);
            let hw = (self.input_size * self.input_size) as usize;
            let mut data = vec![0f32; 3 * hw];
            for y in 0..self.input_size {
                for x in 0..self.input_size {
                    let pixel = resized.get_pixel(x, y);
                    let idx = (y * self.input_size + x) as usize;
                    data[idx] = normalize_channel(pixel.0[0], self.mean[0], self.std[0]);
                    data[hw + idx] = normalize_channel(pixel.0[1], self.mean[1], self.std[1]);
                    data[2 * hw + idx] = normalize_channel(pixel.0[2], self.mean[2], self.std[2]);
                }
            }
            Tensor::from_vec(
                data,
                (1, 3, self.input_size as usize, self.input_size as usize),
                &self.device,
            )
            .map_err(|e| anyhow!("kon inputtensor niet bouwen: {e}"))
        }
    }

    fn normalize_channel(value: u8, mean: f32, std: f32) -> f32 {
        let v = value as f32 / 255.0;
        (v - mean) / std
    }

    struct ClassificationResult {
        present: bool,
        classification: Option<Classification>,
    }
}

pub mod training {
    use anyhow::{Context, Result};
    use std::path::{Path, PathBuf};

    #[derive(Debug, Clone)]
    pub struct DatasetSample {
        pub image_path: PathBuf,
        pub targets: Vec<f32>,
        pub label_index: Option<usize>,
    }

    #[derive(Debug, Clone)]
    pub struct DatasetSplit {
        pub name: String,
        pub samples: Vec<DatasetSample>,
        pub class_names: Vec<String>,
    }

    #[derive(Debug, Clone)]
    pub struct TrainingConfig {
        pub dataset_root: PathBuf,
        pub variant: super::classifier::EfficientNetVariant,
        pub epochs: usize,
        pub batch_size: usize,
        pub learning_rate: f64,
    }

    impl Default for TrainingConfig {
        fn default() -> Self {
            Self {
                dataset_root: PathBuf::from("Voederhuiscamera.v2i.multiclass"),
                variant: super::classifier::EfficientNetVariant::B0,
                epochs: 10,
                batch_size: 32,
                learning_rate: 3e-4,
            }
        }
    }

    /// Load one split (train/valid/test) from a Roboflow export (class CSV + images).
    pub fn load_split(split_dir: impl AsRef<Path>) -> Result<DatasetSplit> {
        let dir = split_dir.as_ref();
        let csv_path = dir.join("_classes.csv");
        let mut rdr = csv::Reader::from_path(&csv_path)
            .with_context(|| format!("kan CSV niet lezen: {}", csv_path.display()))?;
        let headers = rdr
            .headers()
            .context("CSV zonder headers")?
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>();
        if headers.len() < 2 {
            anyhow::bail!("CSV mist klasses: {}", csv_path.display());
        }
        let class_names = headers[1..].to_vec();
        let mut samples = Vec::new();
        for record in rdr.records() {
            let record = record?;
            if record.len() != headers.len() {
                continue;
            }
            let filename = record.get(0).unwrap();
            let mut targets = Vec::with_capacity(class_names.len());
            for value in record.iter().skip(1) {
                targets.push(value.parse::<f32>().unwrap_or(0.0));
            }
            let label_index = targets
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx);
            samples.push(DatasetSample {
                image_path: dir.join(filename),
                targets,
                label_index,
            });
        }
        Ok(DatasetSplit {
            name: dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "split".into()),
            samples,
            class_names,
        })
    }

    /// Convenience helper that loads train/valid/test and logs the counts.
    pub fn load_dataset(
        cfg: &TrainingConfig,
    ) -> Result<(DatasetSplit, DatasetSplit, DatasetSplit)> {
        let train = load_split(cfg.dataset_root.join("train"))?;
        let valid = load_split(cfg.dataset_root.join("valid"))?;
        let test = load_split(cfg.dataset_root.join("test"))?;
        tracing::info!(
            "Dataset geladen: train={} valid={} test={} klassen={}",
            train.samples.len(),
            valid.samples.len(),
            test.samples.len(),
            train.class_names.len()
        );
        Ok((train, valid, test))
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
