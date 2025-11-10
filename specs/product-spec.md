# Feeder Vision — Product Spec (v0.1)
## Problem
Users point to an feeder camera SD card dump folder with thousands of frames; want animal presence + species offline and potentially file and folder reorganization.

## Scope v0
- Single-stage classification pass per frame: generate embeddings (CLIP for v0) and immediately decide `present` + species.
- K-NN over the local gallery/reference pack for species label; same pass also drives the “Aanwezig/Leeg” toggle.
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

## Classification & presence (v0 default)
- Each frame is embedded with CLIP (Candle backend) and compared against the reference gallery via k-NN (HNSW). Presence is inferred directly: any confident species match flips `present=true`.
- Abstain / mark as “Unknown” whenever cos(top1) < T_min or (top1 - top2) < Δ_min.
- Longer-term: allow swapping in a dedicated classifier (e.g., EfficientNet) if CLIP+k-NN accuracy is insufficient; keep the interface the same so GUI/CSV flows do not change.

## UX principles and i18n
- Audience: absolute beginners; UI must be sleek and KISS.
- Do not expose expert options unless strictly necessary; rely on good defaults.
- Primary flow controls only: choose folder, Scan, galerijweergave (Aanwezig | Leeg), Export CSV.
- Dutch-only UI for v0; structure all strings for later multi-language support.
