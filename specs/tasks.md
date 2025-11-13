## Milestone v0 — “Local SD dump to CSV”
- [x] **T0. Repo hygiene**
  - [x] Cargo workspace with `crates/feeder_core` and `crates/app_gui`
  - [x] CI task scripted (`scripts/ci.ps1`) — keep green
  - [x] Spec checks scripted (`scripts/spec_check.ps1`) — scenarios referenced
- [x] **T1. Editor integration (Zed + PowerShell)**
  - [x] `.zed/tasks.json` with “Run CI”, “Check Scenarios”, “Show Progress”
  - [x] ExecutionPolicy set or bypass in tasks (scripts run from Zed)
  - [x] `scripts/progress.ps1` shows overall and per-section %
  - [ ] (Optional) Keybindings for tasks in `keymap.json`

## Core pipeline
- [x] **C0. Public API skeleton (feeder_core)**
  - [x] `scan_folder(path) -> Vec<ImageInfo>`
  - [x] `export_csv(rows, path)`
  - [x] Structs: `ImageInfo`, `Classification`, `Decision`
- [x] **C1. Image ingest**
  - [x] Select folder; list image files (jpg/jpeg/png) (Scenario 1)
  - [x] Empty-folder UX message (Scenario 1)
  - [x] Optional: recursive scan toggle
- [x] **C2. Single-pass EfficientViT inference**
  - [x] Candle EfficientViT classifier wrapper (`ClassifierConfig`, thresholds, background labels)
  - [x] Bundle baseline `feeder-efficientvit-m0.safetensors` + `feeder-labels.csv` in `/models`
  - [x] CPU-side batching + parallel preprocessing for throughput (default batch size = 8)
  - [x] Validate thresholds on feeder SD dumps; document recommended defaults
- [x] **C3. Model training pipeline**
  - [x] Load Roboflow `_classes.csv` splits (train/valid/test) into `DatasetSplit`
  - [x] Training scripts available:
    - Rust CLI (`crates/effnet_train`) for local CPU training (EfficientNet baseline)
    - Google Colab notebook `models/feeder-vision_EfficientViT-training.ipynb` for GPU fine-tuning EfficientViT
  - [x] Export `.safetensors` + label CSV + Colab notebook under `/models`
- [x] **C4. Cropping / preprocessing**
  - [x] Confirm 224×224 pipeline; pad/resize helper for inference + training reuse
- [x] **C5. Open-set safety**
  - [x] Configurable presence threshold & background class list
  - [ ] Unit tests covering Unknown vs species classification
- [ ] **C6. CSV export**
  - [x] `file,present,species,confidence`
  - [x] Disable when no frames selected (Scenario 1)

## GUI (egui)
- [x] **G1. Shell**
  - [x] Main window + folder picker + “Scan” button (Scenario 1)
  - [x] Grid of thumbnails (virtualized) with lazy loading
  - [x] Pre-scan summary count on folder select (e.g., “Afbeeldingen in map: N”)
  - [x] Background scanning worker + progress bar (non-blocking UI)
  - [x] Thumbnail & preview kaarten tonen bestandsnaam + soort + vertrouwen; Windows-style multi-select + Aanwezig/Leeg/Onzeker tabs + standalone viewer
  - [x] Progress bar should reach 100% + keep requesting repaint after scans so thumbnails load without manual interaction (partially met: progress & preview handled, still need auto-repaint).
- [x] **G2. Filters & review**
  - [x] Default view shows only “Aanwezig” (present)
  - [x] Toggle to switch “Aanwezig | Leeg” (Scenario 2 / 2b)
  - [x] “Review unsure” tray (Scenario 3)
- [ ] **G3. Reference manager**
  - [ ] Add to reference → embedding → user index
  - [ ] Species picker (aliases)
  - [ ] Rebuild/compact user index

## Feedback loop
- [x] **F1. Roboflow opt-in uploader**
  - [x] Instellingen checkbox “Help de herkenning te verbeteren” + dataset veld (prefilled met `voederhuiscamera`)
  - [x] Achtergrondthread die manuele herlabelingen (single/multi select) uploadt zonder de UI te blokkeren
  - [x] API-callketen (upload + annotate) gebruikt vaste sleutel en datasetnaam en logt succes/fouten, conform Scenario 5

## Reference packs
- [ ] **R1. Starter pack loader**
  - [ ] Read `/reference/meta.json`; load `index.bin` + overlay `index_user.bin`
- [ ] **R2. Import updates (manual)**
  - [ ] Import `.zip` pack (embeddings + meta), keep user adds separate

## Tests (mapped to scenarios)
- [ ] **S1. Scenario 1: Empty Folder**
  - [ ] e2e test: scan empty → “No images found”; CSV disabled
- [ ] **S2. Scenario 2: Present filter hides empty frames**
  - [ ] e2e test: mixed frames fixture → filter reduces count
- [ ] **S3. Scenario 3: Unknown species abstention**
  - [ ] unit: threshold logic; e2e: out-of-gallery image → “Unknown”
- [ ] **S5. Scenario 5: Opt-in Roboflow upload**
  - [ ] e2e: toggle helpt de herkenning → recategorisatie triggert achtergrondupload en logging

## Packaging
- [ ] **P1. Config & models**
  - [ ] `/models` lookup + friendly missing-model error (weights + labels)
  - [ ] App config (toml/json): thresholds, background labels, batch size
- [ ] **P2. Release**
  - [ ] `cargo build --release`; smoke on Windows 11
  - [ ] (Optional) self-update later via `self_update`
