//! Roboflow upload helper used when sharing manual corrections.

use anyhow::{Context, anyhow};
use reqwest::blocking::{Client, multipart};
use std::path::Path;
use std::time::Duration;

/// Uploads a single image and label pair to Roboflow for improving recognition.
pub fn upload_to_roboflow(
    path: &Path,
    label: &str,
    dataset: &str,
    api_key: &str,
) -> anyhow::Result<()> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());

    let dataset_slug = dataset.trim_matches('/');
    if dataset_slug.is_empty() {
        return Err(anyhow!("Roboflow datasetnaam ontbreekt"));
    }
    let dataset_slug_encoded = urlencoding::encode(dataset_slug);

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("HTTP client bouwen")?;

    let upload_url = format!(
        "https://api.roboflow.com/dataset/{}/upload?api_key={}&name={}&split=train",
        dataset_slug_encoded,
        api_key,
        urlencoding::encode(&filename)
    );

    let form = multipart::Form::new()
        .file("file", path)
        .with_context(|| format!("Bestand toevoegen aan upload-formulier: {}", path.display()))?;

    let response = client
        .post(&upload_url)
        .multipart(form)
        .send()
        .context("Roboflow-upload mislukt")?;
    let status = response.status();
    let response = if status.is_success() {
        response
    } else {
        let body = response
            .text()
            .unwrap_or_else(|_| "<geen body>".to_string());
        return Err(anyhow!(
            "Roboflow-upload gaf een foutstatus: {status} - {body}"
        ));
    };

    let json: serde_json::Value = response
        .json()
        .context("Uploadantwoord kon niet gelezen worden")?;
    let upload_id = json
        .get("id")
        .and_then(|id| id.as_str())
        .or_else(|| {
            json.get("image")
                .and_then(|img| img.get("id"))
                .and_then(|id| id.as_str())
        })
        .ok_or_else(|| anyhow!("Upload-ID ontbreekt in Roboflow-antwoord: {json}"))?;
    tracing::info!("Roboflow-upload voltooid ({upload_id})");

    // Attach a CSV classification annotation so Roboflow applies the selected label.
    let annotate_url = format!(
        "https://api.roboflow.com/dataset/{}/annotate/{}?api_key={}&name={}",
        dataset_slug_encoded,
        urlencoding::encode(upload_id),
        api_key,
        urlencoding::encode("classification.csv")
    );
    let annotation_text = format!("{label}\n");

    let response = client
        .post(&annotate_url)
        .header("Content-Type", "text/plain")
        .body(annotation_text)
        .send()
        .context("Roboflow-annotatie mislukt")?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<geen body>".to_string());
        return Err(anyhow!(
            "Roboflow-annotatie gaf een foutstatus: {status} - {body}"
        ));
    }

    Ok(())
}
