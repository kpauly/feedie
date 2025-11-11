use anyhow::{Context, Result, anyhow};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{Module, Optimizer, ParamsAdamW, VarBuilder, VarMap, loss, optim::AdamW};
use candle_transformers::models::efficientnet::EfficientNet;
use clap::{Parser, ValueEnum};
use feeder_core::{
    ClassifierConfig, EfficientNetVariant, load_image_tensor,
    training::{DatasetSplit, TrainingConfig, load_dataset},
};
use rand::seq::SliceRandom;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum VariantArg {
    B0,
    B1,
    B2,
}

impl From<VariantArg> for EfficientNetVariant {
    fn from(value: VariantArg) -> Self {
        match value {
            VariantArg::B0 => EfficientNetVariant::B0,
            VariantArg::B1 => EfficientNetVariant::B1,
            VariantArg::B2 => EfficientNetVariant::B2,
        }
    }
}

fn default_resolution(variant: EfficientNetVariant) -> u32 {
    match variant {
        EfficientNetVariant::B0 => 224,
        EfficientNetVariant::B1 => 240,
        EfficientNetVariant::B2 => 260,
    }
}

#[derive(Parser)]
#[command(
    name = "effnet-train",
    about = "Fine-tune EfficientNet on the feeder dataset"
)]
struct Args {
    /// Root directory containing train/valid/test folders exported from Roboflow.
    #[arg(long, default_value = "Voederhuiscamera.v2i.multiclass")]
    dataset_root: PathBuf,

    /// Output path for the trained weights (safetensors).
    #[arg(long, default_value = "models/feeder-efficientnet.safetensors")]
    output: PathBuf,

    /// Optional override for the labels file that will be written next to the weights.
    #[arg(long)]
    labels_out: Option<PathBuf>,

    /// Optional path to a pretrained checkpoint used for initialization.
    #[arg(long)]
    pretrained: Option<PathBuf>,

    /// EfficientNet variant to train.
    #[arg(value_enum, long, default_value = "b0")]
    variant: VariantArg,

    /// Override the input resolution (defaults to the canonical value per variant).
    #[arg(long)]
    input_size: Option<u32>,

    /// Batch size used during training.
    #[arg(long, default_value_t = 16)]
    batch_size: usize,

    /// Number of epochs.
    #[arg(long, default_value_t = 5)]
    epochs: usize,

    /// Learning rate for AdamW.
    #[arg(long, default_value_t = 3e-4)]
    learning_rate: f64,
}

struct ImagePipeline<'a> {
    input_size: u32,
    mean: [f32; 3],
    std: [f32; 3],
    device: &'a Device,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    run_training(args)
}

fn run_training(args: Args) -> Result<()> {
    let device = Device::Cpu;
    let variant: EfficientNetVariant = args.variant.into();
    let input_size = args
        .input_size
        .unwrap_or_else(|| default_resolution(variant));
    let base_cfg = ClassifierConfig::default();
    let mean = base_cfg.mean;
    let std = base_cfg.std;

    let data_cfg = TrainingConfig {
        dataset_root: args.dataset_root.clone(),
        variant,
        epochs: args.epochs,
        batch_size: args.batch_size,
        learning_rate: args.learning_rate,
    };
    let (train_split, valid_split, _) = load_dataset(&data_cfg)?;
    if train_split.class_names.is_empty() {
        return Err(anyhow!("geen labels gevonden in dataset"));
    }
    let class_names = train_split.class_names.clone();

    let varmap = VarMap::new();
    let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
    let model = EfficientNet::new(vb, variant.configs(), class_names.len())?;

    let pretrained_path = args
        .pretrained
        .or(Some(base_cfg.model_path))
        .filter(|p| p.exists());
    if let Some(path) = pretrained_path {
        info!("laden van pretrained gewichten uit {}", path.display());
        load_pretrained_partial(&varmap, &path)?;
    } else {
        warn!("geen pretrained gewichten gevonden, training start vanaf random init");
    }

    let adamw_params = ParamsAdamW {
        lr: args.learning_rate,
        ..Default::default()
    };
    let mut optimizer = AdamW::new(varmap.all_vars(), adamw_params)?;
    let pipeline = ImagePipeline {
        input_size,
        mean,
        std,
        device: &device,
    };

    for epoch in 1..=args.epochs {
        let train_loss = train_epoch(
            &model,
            &train_split,
            args.batch_size,
            &pipeline,
            &mut optimizer,
        )?;
        let val_acc = evaluate(&model, &valid_split, args.batch_size, &pipeline)?;
        info!(
            "epoch {:02}: train loss {:.4}, valid acc {:.2}%",
            epoch,
            train_loss,
            val_acc * 100.0
        );
    }

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    varmap.save(&args.output)?;
    info!("gewichten opgeslagen in {}", args.output.display());

    let labels_out = args
        .labels_out
        .unwrap_or_else(|| args.output.with_extension("labels.csv"));
    fs::write(
        &labels_out,
        class_names
            .iter()
            .map(|s| format!("{s}\n"))
            .collect::<String>(),
    )?;
    info!("labels opgeslagen in {}", labels_out.display());
    Ok(())
}

fn train_epoch(
    model: &EfficientNet,
    split: &DatasetSplit,
    batch_size: usize,
    pipeline: &ImagePipeline,
    optimizer: &mut AdamW,
) -> Result<f32> {
    let mut rng = rand::thread_rng();
    let mut indices: Vec<usize> = (0..split.samples.len()).collect();
    indices.shuffle(&mut rng);
    let mut total_loss = 0f32;
    let mut steps = 0usize;
    for chunk in indices.chunks(batch_size) {
        let (images, labels) = batch_from_indices(split, chunk, pipeline)?;
        let logits = model.forward(&images)?;
        let loss = loss::cross_entropy(&logits, &labels)?;
        optimizer.backward_step(&loss)?;
        total_loss += loss.to_scalar::<f32>()?;
        steps += 1;
    }
    Ok(total_loss / steps.max(1) as f32)
}

fn evaluate(
    model: &EfficientNet,
    split: &DatasetSplit,
    batch_size: usize,
    pipeline: &ImagePipeline,
) -> Result<f32> {
    if split.samples.is_empty() {
        return Ok(0.0);
    }
    let mut total_correct = 0f32;
    let mut total = 0usize;
    let all_indices: Vec<usize> = (0..split.samples.len()).collect();
    for chunk in all_indices.chunks(batch_size) {
        let (images, labels) = batch_from_indices(split, chunk, pipeline)?;
        let logits = model.forward(&images)?;
        let preds = logits.argmax(D::Minus1)?;
        let correct = preds
            .eq(&labels)?
            .to_dtype(DType::F32)?
            .sum_all()?
            .to_scalar::<f32>()?;
        total_correct += correct;
        total += labels.dims1()?;
    }
    Ok(total_correct / total.max(1) as f32)
}

fn batch_from_indices(
    split: &DatasetSplit,
    indices: &[usize],
    pipeline: &ImagePipeline,
) -> Result<(Tensor, Tensor)> {
    if indices.is_empty() {
        return Err(anyhow!("lege batch"));
    }
    let mut tensors = Vec::with_capacity(indices.len());
    let mut labels = Vec::with_capacity(indices.len());
    for &idx in indices {
        let sample = split
            .samples
            .get(idx)
            .context("batch index buiten bereik")?;
        let label_idx = sample
            .label_index
            .ok_or_else(|| anyhow!("sample {} mist label", sample.image_path.display()))?;
        let tensor = load_image_tensor(
            &sample.image_path,
            pipeline.input_size,
            pipeline.mean,
            pipeline.std,
            pipeline.device,
        )?;
        tensors.push(tensor);
        labels.push(label_idx as u32);
    }
    let views = tensors.iter().collect::<Vec<_>>();
    let batch = Tensor::stack(&views, 0)?;
    let labels_len = labels.len();
    let labels = Tensor::from_vec(labels, labels_len, pipeline.device)?.to_dtype(DType::U32)?;
    Ok((batch, labels))
}

fn load_pretrained_partial(varmap: &VarMap, path: &Path) -> Result<()> {
    let data = unsafe { candle_core::safetensors::MmapedSafetensors::new(path)? };
    let mut tensor_data = varmap.data().lock().unwrap();
    for (name, var) in tensor_data.iter_mut() {
        match data.load(name, var.device()) {
            Ok(weights) => {
                if weights.shape() == var.shape() {
                    var.set(&weights)?;
                } else {
                    warn!(
                        "vorm komt niet overeen voor {name}: {:?} vs {:?}",
                        weights.shape(),
                        var.shape()
                    );
                }
            }
            Err(err) => warn!("kon tensor {name} niet laden: {err}"),
        }
    }
    Ok(())
}
