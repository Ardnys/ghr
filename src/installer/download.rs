use std::path::PathBuf;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"))
        .join("ghr")
}

pub async fn download_to_cache(
    client: &reqwest::Client,
    url: &str,
    filename: &str,
) -> Result<PathBuf> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create cache dir {}", dir.display()))?;

    let dest = dir.join(filename);

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to GET {url}"))?;

    if !resp.status().is_success() {
        anyhow::bail!("download failed with status {}", resp.status());
    }

    let total = resp.content_length();

    let pb = ProgressBar::new(total.unwrap_or(0));
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    if total.is_none() {
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] {bytes} ({bytes_per_sec})",
            )
            .unwrap(),
        );
    }

    let mut file = tokio::fs::File::create(&dest)
        .await
        .with_context(|| format!("failed to create {}", dest.display()))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("error reading download stream")?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .context("error writing to cache file")?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_and_clear();
    Ok(dest)
}
