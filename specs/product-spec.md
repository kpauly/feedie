# Feeder Vision — Product Spec (v0.1)

## Problem
Users point to a feeder camera SD card dump folder with thousands of frames; they want animal presence + species offline and potentially file/folder reorganization.

## Scope v0
- Single-stage **EfficientViT-m0** classifier (Candle) runs on every frame: resize to 224×224, normalize, batch tensors (default 8) and infer `present` + species directly on CPU.
- Model weights (`feeder-efficientvit-m0.safetensors`) and label CSV (`feeder-labels.csv`) ship with the app under `/models`; users do not need Roboflow/online access.
- Training happens offline from the Roboflow-exported dataset (train/valid/test CSVs) using the Colab notebook (`models/feeder-vision_EfficientViT-training.ipynb`). Updated checkpoints can be dropped into `/models` at any time.
- Open-set behavior relies on classifier confidence + background classes (probability < T_min or predicted label ∈ background → “Unknown”).

## Deliverables
- GUI (egui): folder ingest, grid, review-uncertain tray, “Add to reference”, start with UI in Dutch only, prepare for multi-language support.
- CSV export: `file,present,species,confidence`.
- File reorganization: retain only files with animal presence, sort into species folders.
- Reference pack updater (check for updates, manual import).
- EfficientViT model package: `.safetensors` weights + `labels.csv` shipped with the installer; updated models + training notebook live in `/models`.

## Model training & dataset
- Roboflow export (`Voederhuiscamera.v2i.multiclass/{train,valid,test}`) is the canonical dataset. Each split contains `_classes.csv` (one-hot labels) and preprocessed 512×512 JPGs.
- Training is performed in Google Colab (GPU) using the `feeder-vision_EfficientViT-training.ipynb` notebook; results (best `.safetensors` + labels + metrics) are copied back into `/models`.
- Future improvement: optional “Send misclassified images” workflow that queues new samples for retraining (manual/manual-review only).

## UX Flow (v0)
- Map kiezen (folder picker) toont direct een samenvatting: “Afbeeldingen in map: N”. Er is vóór het scannen geen galerijweergave van alle bestanden.
- Scannen start je expliciet. Tijdens scannen draait detectie op een achtergrondthread; de UI blijft responsief en toont een voortgangsbalk (X/Y, percentage). Knoppen “Kies map…” en “Scannen” zijn uitgeschakeld gedurende het scannen.
- Na scannen toont de UI een samenvatting: “Dieren gevonden in X van Y frames”.
- De galerij toont standaard alleen frames met aanwezigheid (present-only). Er is een snelle schakelaar om te wisselen naar “Leeg” (frames zonder aanwezigheid).
- Thumbnails worden lui (on-demand) geladen met een per-frame limiet om de UI vloeiend te houden bij grote aantallen. Elke kaart toont bestandsnaam + soort + vertrouwen en ondersteunt Windows-achtige selectie (klik, Ctrl/Cmd-klik, Shift-bereik, Ctrl-A) voor contextacties.

## Non-goals v0
- Training, cloud inference, mobile, multi-user sync.

## Performance targets
- 5k frames < 10 min on i5/16GB (no GPU), skipping 80% as empty.
- Reference measurement (i7-1165G7 + Iris Xe, CPU inference): ±12.5 fps while finding 94/217 present frames.

## Classification & presence (v0 default)
- EfficientViT-m0 (fine-tuned on the feeder dataset) loads from `.safetensors` + labels. The classifier threshold controls “Aanwezig” vs “Leeg”.
- “Unknown”/empty is produced when the top probability is below `presence_threshold` or when the winning class is configured as a background label (e.g., “Achtergrond”).
- Longer-term: allow swapping in other Candle classifiers (EfficientNet, ConvNeXt, etc.) without changing the GUI/CSV interface.

## UX principles and i18n
- Audience: absolute beginners; UI must be sleek and KISS.
- Do not expose expert options unless strictly necessary; rely on good defaults.
- Primary flow controls only: choose folder, Scan, galerijweergave (Aanwezig | Leeg), Export CSV.
- Dutch-only UI for v0; structure all strings for later multi-language support.
