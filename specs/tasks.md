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
- [ ] **C2. Single-pass inference pipeline**
  - [ ] CLIP-based embedding + k-NN classification runs once per frame (derives both `present` + species)
  - [ ] Hook to swap in a custom classifier (e.g., EfficientNet) if CLIP accuracy is insufficient
  - [ ] Validate on feeder SD dumps; tune thresholds so “Aanwezig” aligns with observed animals
- [ ] **C3. Cropping**
  - [ ] Pass-through now; square-pad to 224 later
- [ ] **C4. Embeddings (CLIP via Candle)**
  - [ ] Load weights from `/models`; batch embedding
  - [ ] Deterministic preprocessing tests
- [ ] **C5. k-NN search (HNSW)**
  - [ ] Load reference index; query top-k; persist user adds
- [ ] **C6. Open-set safety**
  - [ ] Thresholds: `cos< T_min` or `(top1-top2)< Δ_min` → “Unknown”
  - [ ] Configurable T_min & Δ_min; unit tests
- [ ] **C7. CSV export**
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
  - [ ] `/models` lookup + friendly missing-model error
  - [ ] App config (toml/json): thresholds, batch size
- [ ] **P2. Release**
  - [ ] `cargo build --release`; smoke on Windows 11
  - [ ] (Optional) self-update later via `self_update`
