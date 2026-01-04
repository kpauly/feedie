# Feedie - Product Spec (v1.3)

## Problem
Owners of feeder cameras dump thousands of photos onto their laptops after every SD-card swap. They need a lightweight, offline tool that spots visitors, separates empty frames, and organizes exports without GPU hardware or cloud accounts.

## Scope v1.3
- Single-stage EfficientViT-m0 classifier (Candle) processes each frame (224x224, normalized, batched) entirely on CPU.
- Preprocessing uses zune-jpeg for JPEG decode and fast_image_resize for resizing; PNG uses the image crate.
- Auto-batch tuning picks between baseline 8 and 12 for large scans; preprocessing runs in parallel and pipelines batches (queue depth 2).
- Model weights (feeder-efficientvit-m0.safetensors) and labels (feeder-labels.csv) ship with the app under /models; inference never depends on Roboflow.
- Bundled models are copied to the user data directory on first run; fallback paths cover AppImage AppDir, macOS Resources, and portable layouts.
- Open-set handling relies on confidence plus background labels: probabilities below the presence threshold or labels flagged as background become Unknown.
- Background labels are managed via a dropdown: "achtergrond" always, optional "iets sp." can be treated as background instead of uncertain.
- Optional data sharing: users can enable "Help improve recognition" in settings; manual relabels upload to Roboflow in the background.
- Language support: Dutch, English, French, German, Spanish, Swedish. System auto-detect with manual override.
- Recursive scan toggle in the folder panel.
- In-app updates: model downloads on all platforms; app auto-update for Windows (download installer + launch). macOS/Linux remain manual downloads.

## Deliverables
- egui desktop app: folder ingest, thumbnail grid, uncertain tray, and reference management.
- CSV export containing file,present,species,confidence.
- File reorganization: retain frames with animals and copy them into species folders.
- Context-menu export: the "Export" action on a selection opens a destination picker, creates per-species subfolders, and copies files as <label>_<originalname>.jpg.
- Export tab: dedicated panel with checkboxes for present/uncertain/background photos and CSV generation. Batch export mirrors the gallery structure and, when CSV is enabled, writes metadata (date, time, scientific name, coordinates, path).
- Model updater: check manifest.json, download/install new models into the user data directory.
- Windows updater: download the latest installer and launch it from within the app.
- Linux AppImage packaging with bundled models and desktop entry.
- GitHub Pages website with Dutch and English pages and release-aware download buttons.
- Cached scan results: per-folder cache stored under the user data directory keyed by folder path hash; cached rows load when valid (count/mtime/size).

## Model training and dataset
- Canonical dataset: Voederhuiscamera.v2i.multiclass/{train,valid,test} from Roboflow (512x512 JPGs + _classes.csv).
- GPU fine-tuning via feedie_EfficientViT-training.ipynb in Google Colab; best checkpoints and labels copied back into /models.
- Opt-in uploads feed curated samples into Roboflow for future training runs.

## UX flow (v1.3)
- Folder selection happens in the Photo folder tab with a pre-scan summary (images in folder: N).
- Scans start explicitly; inference runs on a background thread with a progress bar while controls stay disabled.
- After scanning, the UI summarizes "Animals detected in X of Y frames."
- Gallery tabs Present | Empty | Uncertain drive the workflow. Double-click opens a preview window with previous/next controls and a status bar showing label + confidence.
- Thumbnails load lazily with limits so the UI stays smooth; each card shows filename + label + confidence and supports Windows-style selection (click, Ctrl/Cmd-click, Shift range, Ctrl-A). Galleries paginate in slices of 100 cards with navigation controls at the top/bottom and keyboard shortcuts (arrows, Home/End, Page Up/Down) for fast navigation; Shift + navigation extends selection from the anchor.
- Settings expose: presence threshold, background labels dropdown, language selection, recursive scan toggle, and Roboflow opt-in section explaining uploads. Batch size is no longer user-facing.
- Thumbnail context menu order: quick actions (mark background/uncertain), "Export...", explicit label list, "New..." for custom labels.
- Export tab mirrors the rest of the UI and offers checkboxes for Present, Uncertain, Empty, CSV. Pressing Export opens a folder picker and (for CSV) prompts for coordinates before writing voederhuiscamera_yymmddhhmm.csv.

## Non-goals
- Training interface, cloud inference, mobile app, multi-user sync.

## Performance targets
- 5k frames processed in under 10 minutes on an i5/16 GB Windows laptop (no GPU) while skipping at least 80% empty frames.
- Reference measurement (i7-1165G7 + Iris Xe): ~12-20 FPS depending on batch size and IO.

## Classification and presence defaults
- EfficientViT-m0 loads from .safetensors plus labels; the presence threshold separates Present vs Empty.
- Unknown is returned when confidence is below the threshold or the predicted class is a configured background label (for example "Achtergrond" and optionally "Iets sp.").
- Architecture should allow future Candle classifiers without changing the GUI/CSV contract.

## UX principles, i18n and outreach
- Audience: absolute beginners; keep flows simple and avoid expert-only toggles.
- Primary controls: choose folder -> Scan -> review Present/Empty/Uncertain -> Export/CSV.
- Language support for Dutch, English, French, German, Spanish, Swedish, with system auto-detect and manual override.
- Matching website and documentation style so non-technical users can discover installers and instructions quickly.
