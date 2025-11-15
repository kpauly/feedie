<div align="center">
  <img src="assets/Feedie-banner.png" alt="Feedie banner" width="480">
</div>

# Feedie – Efficient bird detection for backyard cameras

[![Release](https://img.shields.io/github/v/release/kpauly/feeder-vision?display_name=tag&logo=github)](https://github.com/kpauly/feeder-vision/releases)
[![Downloads](https://img.shields.io/github/downloads/kpauly/feeder-vision/total)](https://github.com/kpauly/feeder-vision/releases)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-PolyForm%20Noncommercial-blue)](https://polyformproject.org/licenses/noncommercial/1.0.0/)
[![Platform](https://img.shields.io/badge/platform-Windows%2011-blue?logo=windows)](https://github.com/kpauly/feeder-vision/releases)

Feedie is a native Windows application that scans SD-card dumps or folders from your wildlife feeder camera, detects visitors with an EfficientViT model, and helps you export curated images and CSV reports. Everything runs locally—no cloud upload unless you explicitly opt in to sharing samples with Roboflow.

---

## Table of contents

1. [Features](#features)  
2. [Repository structure](#repository-structure)  
3. [Download & install](#download--install)  
4. [Using Feedie](#using-feedie)  
5. [Model & manifest updates](#model--manifest-updates)  
6. [Building from source](#building-from-source)  
7. [Contributing](#contributing)  
8. [License](#license)

---

## Features

- **EfficientViT inference** – Runs the bundled EfficientViT-M0 weights on CPU with configurable thresholds and background classes.
- **Smart galleries** – Tabs for `Aanwezig`, `Leeg`, and `Onzeker` so you can triage detections quickly.
- **Context menu actions** – Assign species, mark background, or create new labels on batches of thumbnails.
- **Export workflows** – Per-selection export via context menu or batch export from the dedicated tab (species folders, Onzeker bundle, Leeg bundle, CSV with metadata).
- **Model updater** – Checks `manifest.json`, shows available app/model versions, and can download/install new models automatically into `%AppData%\Feedie\models`.
- **Roboflow opt-in uploader** – When enabled, manual relabels are uploaded in the background without blocking the UI.

---

## Repository structure

```
.
├── assets/                # Branding and installer imagery
├── manifest.json          # App + model version manifest consumed by the updater
├── models/                # Bundled EfficientViT weights, labels, and version file
├── crates/
│   ├── app_gui/           # egui desktop application (“Feedie”)
│   └── feeder_core/       # Core inference, scanning, and CSV utilities
├── scripts/               # CI helper scripts (fmt, clippy, tests, spec checks)
└── specs/                 # Product spec, tasks, and acceptance scenarios
```

- `crates/app_gui`: Uses `eframe/egui` for the Windows UI, handles scanning, manifest fetching, and model download/installation.
- `crates/feeder_core`: Library with `scan_folder_with`, `EfficientVitClassifier`, and export helpers, reusable in other tools.

---

## Download & install

1. Grab the latest `FeedieSetup.exe` from the [GitHub releases](https://github.com/kpauly/feeder-vision/releases).
2. Run the installer (administrator privileges required). Feedie is installed to `C:\Program Files\Feedie`.
3. Launch Feedie from the Start menu. The bundled model is copied to `%AppData%\Feedie\models` on first run so you can work offline immediately.

> Prefer a portable build? Run `cargo build --release -p app_gui` and start `target\release\Feedie.exe`.

---

## Using Feedie

1. **Fotomap tab** – Point Feedie at your SD-card folder. It shows how many frames were found and lets you start a scan.
2. **Scanresultaat tab** – Review `Aanwezig`, `Leeg`, and `Onzeker` galleries. Use the context menu for quick relabels or double-click to open the preview window.
3. **Exporteren tab** – Choose what to export:
   - Species with confident detections (subfolders per species)
   - All `Onzeker` samples (single `Onzeker` folder)
   - All `Leeg` frames (single `Leeg` folder)
   - CSV with date/time/scientific name/lat/lng/path (coordinates are prompted once per export)
4. **Instellingen tab** – Adjust thresholds, enable Roboflow uploads, and manage updates. The section at the bottom shows app/model versions and exposes the “Download en installeren” button when a new model is published.

Documentation in `specs/` covers the product spec, tasks, and test scenarios if you want a deeper dive.

---

## Model & manifest updates

- `manifest.json` is the lightweight descriptor hosted via GitHub raw. It contains the latest app tag and model release.
- Feedie stores active models in `%AppData%\Feedie\models`, always expecting three files:
  - `feeder-efficientvit-m0.safetensors`
  - `feeder-labels.csv`
  - `model_version.txt`
- When the manifest reports a newer model version, the UI offers a download button. Feedie fetches the ZIP (`Feedie_EfficientViT-m0_vX.Y.Z.zip`), validates the contents, installs them into the AppData directory, refreshes the labels, and updates `model_version.txt`.
- You can also unzip releases manually into `%AppData%\Feedie\models` if you prefer fully offline installs.

If you launch without network access, Feedie keeps using the installed model. Click “Opnieuw controleren” once you reconnect.

---

## Building from source

Prerequisites:

- Rust toolchain (`rustup` with the MSVC target on Windows)
- Windows 11 build tools (Visual Studio Build Tools or Desktop Development with C++)

Steps:

```powershell
git clone https://github.com/kpauly/feeder-vision.git
cd feeder-vision
./scripts/ci.ps1       # fmt + clippy + tests
cargo run -p app_gui   # debug build
cargo run --release -p app_gui
```

CI helper scripts:

- `./scripts/ci.ps1` – format + clippy + tests
- `./scripts/spec_check.ps1` – ensures every scenario in `specs/scenarios.md` is referenced by tests

---

## Contributing

We welcome contributions and feedback! Ideas:

- Improve EfficientViT training (see `models/` + `specs/tasks.md`)
- Polish UI/UX or translations
- Add new export formats or automation scripts
- Build integrations using `feeder_core`

Before submitting a PR:

1. Update specs/tests if your change affects user-facing behavior.
2. Run `./scripts/ci.ps1`.
3. Describe which scenario/task you’re targeting so we can keep specs aligned.

Questions? Open an issue or start a discussion.

---

## License

Feedie is distributed under the [PolyForm Noncommercial License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/). You’re welcome to use and modify the app for noncommercial purposes. For commercial licensing, please reach out to the maintainers.

---

Happy birding! 🐦📸 If you build something on top of Feedie—new models, workflows, or accessories—we’d love to hear about it.
