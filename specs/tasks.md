## Milestone v1.3 - Local SD dump to CSV + multilingual + updates

- [x] **T0. Repo hygiene**
  - [x] Cargo workspace with crates/feeder_core and crates/app_gui
  - [x] CI task scripted (scripts/ci.ps1)
  - [x] Spec checks scripted (scripts/spec_check.ps1)
- [x] **T1. Editor integration (Zed + PowerShell)**
  - [x] .zed/tasks.json with CI and spec checks

## Core pipeline
- [x] **C0. Public API skeleton (feeder_core)**
  - [x] scan_folder(path) -> Vec<ImageInfo>
  - [x] export_csv(rows, path)
  - [x] Structs: ImageInfo, Classification, Decision
- [x] **C1. Image ingest**
  - [x] Select folder; list jpg/jpeg/png files (Scenario 1)
  - [x] Empty-folder UX ("No images found")
  - [x] Recursive scan toggle
- [x] **C2. EfficientViT inference**
  - [x] Candle EfficientViT wrapper (ClassifierConfig, thresholds, background labels)
  - [x] Bundle feeder-efficientvit-m0.safetensors + feeder-labels.csv in /models
  - [x] CPU batching with auto-tuning (baseline 8, optional 12)
  - [x] Parallel preprocessing + pipelined batches (queue depth 2)
- [x] **C3. Model training pipeline**
  - [x] Roboflow dataset split ingestion
  - [x] Training notebook: models/feedie_EfficientViT-training.ipynb
  - [x] Export .safetensors and label CSV
- [x] **C4. Preprocessing**
  - [x] zune-jpeg decode for JPEG
  - [x] fast_image_resize for SIMD resize
- [x] **C5. Open-set safety**
  - [x] Configurable presence threshold + background list
  - [ ] Follow-up: unit tests for Unknown vs species classification
- [x] **C6. Export**
  - [x] file,present,species,confidence CSV
  - [x] Context export and batch export flows
  - [ ] Follow-up: export progress/reporting
- [x] **C7. Cached scan results**
  - [x] Per-folder cache stored under user data directory
  - [x] Cache validation (count/mtime/size)
  - [x] Cache load with manual edits preserved

## GUI (egui)
- [x] **G1. Shell**
  - [x] Main window + folder picker + Scan button
- [x] **G2. Results grid**
  - [x] Present/Empty/Uncertain tabs with pagination and keyboard navigation
  - [x] Lazy thumbnails with load progress label
  - [x] Preview window with navigation
- [x] **G3. Context menu**
  - [x] Export on selection
  - [x] Mark background / mark something sp.
  - [x] New label entry
- [x] **G4. Export tab**
  - [x] Present/Uncertain/Empty/CSV checkboxes and coordinate prompt
- [x] **G5. Settings**
  - [x] Presence threshold + recompute
  - [x] Background labels dropdown (achtergrond + optional iets sp.)
  - [x] Language selection (system + manual list)
  - [x] Recursive scan toggle
  - [x] Roboflow opt-in + dataset field
- [ ] **G6. Reference manager**
  - [ ] Embeddings and user index (future)

## Feedback loop
- [x] **F1. Roboflow opt-in uploader**
  - [x] Settings toggle + dataset name
  - [x] Background upload on relabel without blocking UI

## Tests (mapped to scenarios)
- [ ] **S1. Empty folder**
- [ ] **S2. Present/Empty filters**
- [ ] **S3. Unknown/Background behavior**
- [ ] **S5. Roboflow upload**
- [ ] **S7. Export selected thumbnails**
- [ ] **S8. Export tab batch export**
- [ ] **S9. Cached scan**

## Packaging
- [x] **P1. Config & models**
  - [x] Bundle /models and deploy to AppData with fallback
  - [x] Persisted app settings (language/background/recursive)
- [x] **P2. Release builds**
  - [x] Windows installer (Inno Setup)
  - [x] macOS Intel + Apple Silicon zips
  - [x] Linux AppImage + workflow
- [x] **P3. Manifest & updater**
  - [x] manifest.json hosted on GitHub raw
  - [x] In-app model download/install
  - [x] Windows auto-update (download installer + launch)
- [x] **P4. Website**
  - [x] GitHub Pages site with latest release buttons
  - [x] Dutch + English pages

## Performance follow-ups
- [ ] Optional BLAS backend (MKL/Accelerate/OpenBLAS) if ROI justifies complexity
