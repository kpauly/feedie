# Feeder Vision — Product Spec (v0.1)
## Problem
Users point to an feeder camera SD card dump folder with thousands of frames; want animal presence + species offline and potentially file and folder reorganization.

## Scope v0
- Stage A: optional “animal present” filter (YOLO-n or heuristic motion).
- Stage B3: CLIP embeddings + HNSW k-NN over local gallery.
- Open-set: abstain if cos(top1) < T_min or (top1-top2) < Δ_min.

## Deliverables
- GUI (egui): folder ingest, grid, review-uncertain tray, “Add to reference”, start with UI in Dutch only, prepare for multi-language support.
- CSV export: file,present,species,confidence.
- File reorganization: retain only files with animal presence, sort into species folders.
- Reference pack updater (check for updates, manual import).

## UX Flow (v0)
- Map kiezen (folder picker) toont direct een samenvatting: "Afbeeldingen in map: N". Er is vóór het scannen geen galerijweergave van alle bestanden.
- Scannen start je expliciet. Tijdens scannen draait detectie op een achtergrondthread; de UI blijft responsief en toont een voortgangsbalk (X/Y, percentage). Knoppen "Kies map…" en "Scannen" zijn uitgeschakeld gedurende het scannen.
- Na scannen toont de UI een samenvatting: "Dieren gevonden in X van Y frames".
- De galerij toont standaard alleen frames met aanwezigheid (present-only). Er is een snelle schakelaar om te wisselen naar "Leeg" (frames zonder aanwezigheid).
- Thumbnails worden lui (on‑demand) geladen met een per‑frame limiet om de UI vloeiend te houden bij grote aantallen.

## Non-goals v0
- Training, cloud inference, mobile, multi-user sync.

## Performance targets
- 5k frames < 10 min on i5/16GB (no GPU), skipping 80% as empty.

## Presence detection (v0 default)
- Use background-difference detection based on a compact image hash (64-bit dHash) and K=2 clustering in Hamming space to handle day/night modes.
- Decide "present" for frames that are outliers relative to their assigned background cluster using an automatic threshold (mean + k·std; k≈2.5), with no user-visible tuning.
- Keep a future option to swap in a YOLO-based detector behind a feature flag if MVP results are insufficient.

## UX principles and i18n
- Audience: absolute beginners; UI must be sleek and KISS.
- Do not expose expert options unless strictly necessary; rely on good defaults.
- Primary flow controls only: choose folder, Scan, galerijweergave (Aanwezig | Leeg), Export CSV.
- Dutch-only UI for v0; structure all strings for later multi-language support.
