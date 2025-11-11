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
- [ ] **C1. Image ingest**
  - [x] Select folder; list image files (jpg/jpeg/png) (Scenario 1)
  - [x] Empty-folder UX message (Scenario 1)
  - [x] Optional: recursive scan toggle
- [ ] **C2. Single-pass EfficientNet inference**
  - [x] Candle EfficientNet classifier wrapper (`ClassifierConfig`, thresholds, background labels)
  - [ ] Bundle baseline `.safetensors` + `labels.csv` in `/models`
  - [ ] Validate thresholds on feeder SD dumps; document recommended defaults
- [ ] **C3. Model training pipeline**
  - [x] Load Roboflow `_classes.csv` splits (train/valid/test) into `DatasetSplit`
  - [ ] Candle training script (data loader, augmentations, EfficientNet fine-tune loop)
  - [ ] Export `.safetensors` + metrics artifact; hook into `/models`
- [ ] **C4. Cropping / preprocessing**
  - [ ] Confirm 512×512 pipeline; pad/resize helper for inference + training reuse
- [ ] **C5. Open-set safety**
  - [ ] Configurable presence threshold & background class list
  - [ ] Unit tests covering Unknown vs species classification
- [ ] **C6. CSV export**
  - [x] `file,present,species,confidence`
  - [x] Disable when no frames selected (Scenario 1)

## GUI (egui)
- [ ] **G1. Shell**
  - [x] Main window + folder picker + “Scan” button (Scenario 1)
  - [x] Grid of thumbnails (virtualized)
  - [x] Pre-scan summary count on folder select (e.g., “Afbeeldingen in map: N”)
  - [x] Background scanning worker + progress bar (non-blocking UI)
  - [ ] Progress bar should reach 100% + keep requesting repaint after scans so thumbnails load without manual interaction
- [ ] **G2. Filters & review**
  - [x] Default view shows only “Aanwezig” (present)
  - [x] Toggle to switch “Aanwezig | Leeg” (Scenario 2 / 2b)
  - [ ] “Review unsure” tray (Scenario 3)
- [ ] **G3. Reference manager**
  - [ ] Add to reference → embedding → user index
  - [ ] Species picker (aliases)
  - [ ] Rebuild/compact user index

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

## Packaging
- [ ] **P1. Config & models**
  - [ ] `/models` lookup + friendly missing-model error (weights + labels)
  - [ ] App config (toml/json): thresholds, background labels, batch size
- [ ] **P2. Release**
  - [ ] `cargo build --release`; smoke on Windows 11
  - [ ] (Optional) self-update later via `self_update`
