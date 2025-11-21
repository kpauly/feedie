## Milestone v0 – “Local SD dump to CSV”

- [x] **T0. Repo hygiene**
  - [x] Cargo workspace with `crates/feeder_core` and `crates/app_gui`
  - [x] CI task scripted (`scripts/ci.ps1`) – keep green
  - [x] Spec checks scripted (`scripts/spec_check.ps1`) – scenarios referenced
- [x] **T1. Editor integration (Zed + PowerShell)**
  - [x] `.zed/tasks.json` with “Run CI”, “Check Scenarios”, “Show Progress”
  - [x] Execution policy bypassed in tasks so scripts run directly from Zed
  - [x] `scripts/progress.ps1` shows overall and per-section %
  - [ ] (Optional) Task keybindings in `keymap.json`

## Core pipeline
- [x] **C0. Public API skeleton (feeder_core)**
  - [x] `scan_folder(path) -> Vec<ImageInfo>`
  - [x] `export_csv(rows, path)`
  - [x] Structs: `ImageInfo`, `Classification`, `Decision`
- [x] **C1. Image ingest**
  - [x] Select folder; list jpg/jpeg/png files (Scenario 1)
  - [x] Empty-folder UX (“No images found”)
  - [x] Optional: recursive scan toggle
- [x] **C2. Single-pass EfficientViT inference**
  - [x] Candle EfficientViT wrapper (`ClassifierConfig`, thresholds, background labels)
  - [x] Bundle baseline `feeder-efficientvit-m0.safetensors` + `feeder-labels.csv` in `/models`
  - [x] CPU batching + parallel preprocessing (default batch size = 8)
  - [x] Validate thresholds on feeder dumps; document defaults
- [x] **C3. Model training pipeline**
  - [x] Load Roboflow `_classes.csv` splits into `DatasetSplit`
  - [x] Training scripts: Google Colab notebook `models/feedie_EfficientViT-training.ipynb`
  - [x] Export `.safetensors`, label CSV, and notebook under `/models`
- [x] **C4. Cropping / preprocessing**
  - [x] Confirm 224×224 pipeline; reusable pad/resize helper
- [x] **C5. Open-set safety**
  - [x] Configurable presence threshold & background list
  - [ ] Unit tests covering Unknown vs species classification
- [x] **C6. CSV export**
  - [x] `file,present,species,confidence`
  - [x] Disable when nothing selected (Scenario 1)
  - [ ] (Follow-up) Export progress + warnings when labels are missing

## GUI (egui)
- [x] **G1. Shell**
  - [x] Main window + folder picker + “Scan” button (Scenario 1)
  - [x] Virtualised thumbnail grid with lazy loading
  - [x] Pre-scan summary (“Images in folder: N”)
  - [x] Background scanning worker + progress bar
  - [x] Thumbnail & preview cards show filename + species + confidence; Windows-style multi-select; Present/Empty/Uncertain tabs; standalone viewer
  - [x] Progress bar reaches 100% and triggers repaints so thumbnails keep loading
- [x] **G2. Filters & review**
  - [x] Default view = Present only
  - [x] Toggle between Present | Empty (Scenario 2/2b)
  - [x] “Review unsure” tray (Scenario 3)
- [x] **G4. Context-menu export**
  - [x] Export entry at the top of the thumbnail menu with sub-menu arrow
  - [x] Folder picker + per-species subfolders and `<label>_<original>.jpg`
  - [ ] (Follow-up) Export progress + warnings for missing labels
- [x] **G5. Export tab**
  - [x] Checkboxes for present/uncertain/background photos + CSV option
  - [x] Export button opens folder picker and (for CSV) coordinate prompt
  - [x] Subfolders per chosen category + batch copy naming scheme
  - [x] Subfolder logic mirrors galleries (Present ⇒ per species, Uncertain, Empty)
  - [x] CSV `voederhuiscamera_yymmddhhmm.csv` with `date,time,scientific name,lat,lng,path`
  - [ ] (Follow-up) Batch export progress/reporting
- [ ] **G3. Reference manager**
  - [ ] Add-to-reference flow → embeddings → user index
  - [ ] Species picker (aliases)
  - [ ] Rebuild/compact user index

## Feedback loop
- [x] **F1. Roboflow opt-in uploader**
  - [x] Settings checkbox “Help improve recognition” + dataset field (`voederhuiscamera`)
  - [x] Background thread uploads manual relabels (single/multi select) without blocking UI
  - [x] Upload + annotate API chain logs success/errors (Scenario 5)
  - [x] Context menu “New…” input pushes cards to Present and uploads new labels (Scenario 6)

## Reference packs
- [ ] **R1. Starter pack loader** – read `/reference/meta.json`, load `index.bin`, overlay `index_user.bin`
- [ ] **R2. Import updates** – import `.zip` pack (embeddings + meta) while keeping user additions separate

## Tests (mapped to scenarios)
- [ ] **S1. Empty folder** – e2e: scan empty → “No images found”, CSV disabled
- [ ] **S2. Present filter hides empty frames** – e2e: mixed fixture → filter reduces count
- [ ] **S3. Unknown species abstention** – unit: threshold logic; e2e: out-of-gallery image → “Unknown”
- [ ] **S5. Opt-in Roboflow upload** – e2e: toggle helper → relabel triggers background upload + logging
- [ ] **S7. Export selected thumbnails** – e2e: multi-select → context-menu export places files into species folders with correct naming
- [ ] **S8. Export tab batch export** – e2e: check options → choose folder → enter coordinates → subfolders + CSV created

## Packaging
- [x] **P1. Config & models**
  - [x] Bundle `/models` and deploy to AppData with fallback
  - [ ] Persisted app config (toml/json) for thresholds/backgrounds/batch size
- [x] **P2. Release**
  - [x] `cargo build --release`; smoke test on Windows 11
  - [x] Windows installer (`FeedieSetup.exe`) with bundled model
  - [x] Universal macOS build (`Feedie-mac.zip`) via GitHub Actions
  - [ ] (Optional) self-update via `self_update`
- [x] **P3. Manifest & updater**
  - [x] `manifest.json` hosted on GitHub raw
  - [x] In-app version display + manifest fetch with retry/error states
  - [x] Automatic model ZIP download/extract into `%AppData%\Feedie\models`
  - [ ] (Follow-up) App auto-update prompt/link to latest installer
- [x] **P4. Website & releases**
  - [x] GitHub Pages site (`docs/`) in Dutch with buttons that resolve to the latest release assets
  - [x] Publish both installers under the same tag with matching release notes
