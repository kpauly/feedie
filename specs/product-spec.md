# Feedie - Product Spec (v0.1)

## Problem
Owners of feeder cameras dump thousands of photos onto their laptops after every SD-card swap. They need a lightweight, offline tool that spots visitors, separates empty frames, and organises exports without GPU hardware or cloud accounts.

## Scope v0
- Single-stage **EfficientViT-m0** classifier (Candle) processes each frame (224x224, normalised, batched; default batch size 8) entirely on CPU.
- Model weights (`feeder-efficientvit-m0.safetensors`) and labels (`feeder-labels.csv`) ship with the app under `/models`; inference never depends on Roboflow.
- Training happens offline using the Roboflow export (`Voederhuiscamera.v2i.multiclass`) and the Colab notebook `models/feedie_EfficientViT-training.ipynb`. Updated checkpoints can be copied into `/models` at any time.
- Open-set handling relies on classifier confidence plus background labels: probabilities below the presence threshold or labels flagged as background become "Unknown".
- Opt-in data sharing: users can enable "Help improve recognition" in the settings panel; manual relabels are uploaded to Roboflow without blocking the UI.

## Deliverables
- egui desktop app: folder ingest, thumbnail grid, uncertain tray, and basic reference management; UI starts in Dutch with future localisation in mind.
- CSV export containing `file,present,species,confidence`.
- File reorganisation: retain only frames with animals and copy them into species folders.
- Context-menu export: the "Export" action on a selection opens a destination picker, creates per-species subfolders, and copies files as `<label>_<originalname>.jpg`.
- Export tab: dedicated panel with checkboxes for present/uncertain/background photos and CSV generation. Batch export mirrors the gallery structure and, when CSV is enabled, writes metadata (date, time, scientific name, coordinates, path).
- Reference pack updater (check for model/reference updates and allow manual import).
- EfficientViT model package distributed with both the Windows installer and macOS `.app` bundle; refreshed models plus training notebook live in `/models`.
- Roboflow feedback toggle: built-in API key, dataset field, background uploader for every manual relabel.
- Context menu "New..." entry so users can add custom species labels on the fly; selections move to Present and are uploaded (when enabled).
- Dutch GitHub Pages site with auto-updating download buttons for Windows and macOS.

## Model training & dataset
- Canonical dataset: `Voederhuiscamera.v2i.multiclass/{train,valid,test}` from Roboflow (512x512 JPGs + `_classes.csv`).
- GPU fine-tuning via `feedie_EfficientViT-training.ipynb` in Google Colab; best checkpoints + labels copied back into `/models`.
- Opt-in uploads feed curated samples straight into Roboflow, ready for future training runs.

## UX flow (v0)
- Folder selection happens in the **Photo folder** tab, which shows the path and "Images in this folder: N" and lets users switch tabs without re-running operations.
- Scans start explicitly; inference runs on a background thread with a progress bar (X/Y, percentage) while "Choose folder" and "Scan" stay disabled.
- After scanning, the UI summarises "Animals detected in X of Y frames".
- Gallery tabs **Present | Empty | Uncertain** drive the workflow. Double-click opens a dedicated preview window with previous/next controls and a status bar showing label + confidence.
- Present contains high-confidence species (including manual overrides), Uncertain groups low-confidence hits ("something sp."), Empty groups background frames.
- Thumbnails load lazily with limits so the UI stays smooth; each card shows filename + label + confidence and supports Windows-style selection (click, Ctrl/Cmd-click, Shift range, Ctrl-A).
- Settings exposes presence threshold, batch size, background labels, and the Roboflow opt-in section explaining uploads.
- Thumbnail context menu order: quick actions (mark background/uncertain), "Export...", explicit label list, "New..." for custom labels.
- Export tab mirrors the rest of the UI and offers checkboxes for Present, Uncertain, Empty, CSV. Pressing "Export" opens a folder picker, creates per-gallery subfolders, and - when CSV is enabled - prompts for coordinates before writing `voederhuiscamera_yymmddhhmm.csv`.

## Non-goals
- Training interface, cloud inference, mobile app, multi-user sync.

## Performance targets
- 5k frames processed in under 10 minutes on an i5/16 GB Windows laptop (no GPU) while skipping at least 80% empty frames.
- Reference measurement (i7-1165G7 + Iris Xe): ~12.5 FPS, detecting 94/217 present frames.

## Classification & presence defaults
- EfficientViT-m0 loads from `.safetensors` + labels; the presence threshold separates Present vs Empty.
- "Unknown" is returned when confidence is below the threshold or the predicted class is a configured background label (e.g. "Achtergrond").
- Architecture should allow future Candle classifiers (ConvNeXt, etc.) without changing the GUI/CSV contract.

## UX principles, i18n & outreach
- Audience: absolute beginners; keep flows simple and avoid expert-only toggles.
- Primary controls: choose folder -> Scan -> review Present/Empty/Uncertain -> Export/CSV.
- Dutch-only interface for v0, but strings are structured for future translations.
- Matching website and documentation style so non-technical users can discover installers and instructions quickly.
