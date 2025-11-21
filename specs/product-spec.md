# Feedie - Product Spec (v0.1)

## Problem
Users point to a feeder camera SD card dump folder with thousands of frames; they want animal presence + species offline and potentially file/folder reorganization on a modest Windows laptop or Mac.

## Scope v0
- Single-stage **EfficientViT-m0** classifier (Candle) runs on every frame: resize to 224x224, normalize, batch tensors (default 8) and infer `present` + species directly on CPU.
- Model weights (`feeder-efficientvit-m0.safetensors`) and label CSV (`feeder-labels.csv`) ship with the app under `/models`; users do not need Roboflow/online access.
- Training happens offline from the Roboflow-exported dataset (train/valid/test CSVs) using the Colab notebook (`models/feedie_EfficientViT-training.ipynb`). Updated checkpoints can be dropped into `/models` at any time.
- Open-set behavior relies on classifier confidence plus background classes (probability < T_min or predicted label "background" -> "Unknown").
- Opt-in data sharing: via Instellingen users can enable "Help de herkenning te verbeteren", which uploads manually re-categorised frames in the background to the Roboflow project without blocking the UI.

## Deliverables
- GUI (egui): folder ingest, grid, review-uncertain tray, "Add to reference"; start with UI in Dutch only, prepare for multi-language support.
- CSV export: `file,present,species,confidence`.
- File reorganization: retain only files with animal presence, sort into species folders.
- Snelle contextmenu-export: "Exporteren" bovenaan elke selectie laat je een doelmap kiezen, maakt submappen per soort aan en kopieert de geselecteerde beelden als `<soort>_<originele bestandsnaam>.jpg`.
- Exporttab: aparte "Exporteren"-paneel met checkboxen voor aanwezige/onzekere/achtergrondfoto's en CSV-export; batch export creëert submappen en kan tegelijk een CSV met datum, tijd, wetenschappelijke naam, coördinaten en pad genereren.
- Reference pack updater (check for updates, manual import).
- EfficientViT model package: `.safetensors` weights + `labels.csv` shipped with both Windows installer en macOS app bundle; updated models + training notebook live in `/models`.
- Roboflow feedback toggle: built-in API key + dataset field, with a background uploader that pushes every manual re-labelling (single or multi select) to dataset `voederhuiscamera`.
- Contextmenu bevat een “Nieuw…”-optie waarmee gebruikers on-the-fly een nieuwe soortnaam kunnen ingeven; de selectie krijgt meteen het manuele label en (indien upload is ingeschakeld) wordt samen met het label naar Roboflow verstuurd.
- Nederlandstalige productwebsite (GitHub Pages) met downloads die automatisch naar de laatste release verwijzen.

## Model training & dataset
- Roboflow export (`Voederhuiscamera.v2i.multiclass/{train,valid,test}`) is the canonical dataset. Each split contains `_classes.csv` (one-hot labels) and preprocessed 512x512 JPGs.
- Training is performed in Google Colab (GPU) using the `feedie_EfficientViT-training.ipynb` notebook; results (best `.safetensors` + labels + metrics) are copied back into `/models`.
- The opt-in uploader now feeds fresh samples straight into the Roboflow dataset so future training runs can consume manually curated examples without extra tooling.

## UX Flow (v0)
- Map kiezen gebeurt via het **Fotomap**-paneel: toont pad + "Afbeeldingen in deze map: N" en laat schakelen tussen Fotomap, Scanresultaat en Instellingen zonder acties opnieuw te starten.
- Scannen start je expliciet. Tijdens scannen draait detectie op een achtergrondthread; de UI blijft responsief en toont een voortgangsbalk (X/Y, percentage). Knoppen "Kies map" en "Scannen" zijn uitgeschakeld gedurende het scannen.
- Na scannen toont de UI een samenvatting: "Dieren gevonden in X van Y frames".
- De galerij toont tabs **Aanwezig | Leeg | Onzeker**; dubbelklikken opent een los previewvenster met Vorige/Volgende (ook pijltjes) en statusbalk (label + confidence).
- De galerij toont tabs **Aanwezig | Leeg | Onzeker**; Aanwezig bevat alleen hoge-confidence soorten (inclusief manuele toewijzingen), Onzeker bundelt "Iets sp." en lage confidence, Leeg groepeert achtergrondframes. Dubbelklikken opent een los previewvenster met Vorige/Volgende (ook pijltjes) en statusbalk (label + confidence).
- Thumbnails worden lui (on-demand) geladen met een per-frame limiet om de UI vloeiend te houden bij grote aantallen. Elke kaart toont bestandsnaam + soort + vertrouwen en ondersteunt Windows-achtige selectie (klik, Ctrl/Cmd-klik, Shift-bereik, Ctrl-A) voor contextacties.
- Instellingen bevat sliders voor onzekerheidsdrempel, batchgrootte, achtergrondlabels en de nieuwe sectie "Help de herkenning te verbeteren", inclusief datasetveld en uitleg dat uploads volledig op de achtergrond plaatsvinden zodra de checkbox aan staat.
- Het contextmenu onder thumbnails toont eerst quick actions (achtergrond, onzeker), daarna "Exporteren ›" (opent een mapkiezer en kopieert de selectie per soortmap), vervolgens de bestaande soorten en sluit af met “Nieuw… >” waar de gebruiker een eigen label kan invullen; na bevestiging verschuift de kaart automatisch naar Aanwezig en, indien van toepassing, triggert een Roboflow-upload.
- Het tabblad **Exporteren** biedt dezelfde look & feel als Fotomap/Scanresultaat en toont vier opties (aanwezige soorten, onzekere identificatie, achtergrond/Leeg, CSV). Bij een klik op “Exporteer” kiest de gebruiker een doelmap, waarna de app submappen maakt die overeenkomen met de galerijen (één map per soort in Aanwezig, één map "Onzeker", één map "Leeg") en – indien CSV aan staat – om Google Maps-coördinaten vraagt om vervolgens een `voederhuiscamera_yymmddhhmm.csv` met datum, tijd, wetenschappelijke naam, lat/lng en pad naar elke geëxporteerde foto te schrijven.

## Non-goals v0
- Training, cloud inference, mobile, multi-user sync.

## Performance targets
- 5k frames < 10 min on i5/16GB (no GPU), skipping 80% as empty.
- Reference measurement (i7-1165G7 + Iris Xe, CPU inference): ~12.5 fps while finding 94/217 present frames.

## Classification & presence (v0 default)
- EfficientViT-m0 (fine-tuned on the feeder dataset) loads from `.safetensors` + labels. The classifier threshold controls "Aanwezig" vs "Leeg".
- "Unknown"/empty is produced when the top probability is below `presence_threshold` or when the winning class is configured as a background label (e.g., "Achtergrond").
- Longer-term: allow swapping in other Candle classifiers (ConvNeXt, etc.) without changing the GUI/CSV interface.

## UX principles, i18n & outreach
- Audience: absolute beginners; UI must be sleek and KISS.
- Do not expose expert options unless strictly necessary; rely on good defaults.
- Primary flow controls only: choose folder, Scan, galerijweergave (Aanwezig | Leeg), Export CSV.
- Dutch-only UI for v0; structure all strings for later multi-language support.
- Begeleidende website in dezelfde stijl (Feedie blauw/bruin) zodat niet-technische gebruikers direct installatielinks en korte uitleg vinden.
