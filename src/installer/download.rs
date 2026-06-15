use std::path::PathBuf;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"))
        .join("ghr")
}

/// Create a styled progress bar, optionally pre-attached to a `MultiProgress`.
///
/// Pass `Some(mp)` when building bars for concurrent downloads: the bar is added to `mp`
/// *before* any setters are called, so there are no orphaned draws to the raw terminal.
/// Pass `None` for single-tool installs where no `MultiProgress` is involved.
pub fn make_progress_bar(
    mp: Option<&MultiProgress>,
    total: u64,
    prefix: impl Into<String>,
    prefix_len: Option<usize>,
) -> ProgressBar {
    let pb = match mp {
        Some(mp) => mp.add(ProgressBar::new(total)),
        None => ProgressBar::new(total),
    };
    let style = if total > 0 {
        ProgressStyle::with_template(
            "{prefix:.bold}  {bar:45.green/black.dim}  {bytes:>10} / {total_bytes:<10}  {bytes_per_sec:>12}  eta {eta}",
        )
        .unwrap()
        .progress_chars("━──")
    } else {
        ProgressStyle::with_template("{prefix:.bold}  {spinner:.green}  {bytes}  {bytes_per_sec}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""])
    };
    pb.set_style(style);
    let prefix = prefix.into();
    let padded = match prefix_len {
        Some(w) => format!("{:<width$}", prefix, width = w),
        None => prefix,
    };
    pb.set_prefix(padded);
    pb
}

pub async fn download_to_cache(
    client: &reqwest::Client,
    url: &str,
    filename: &str,
    pb: ProgressBar,
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

    // Refine bar length with actual content-length if asset metadata was off.
    if let Some(len) = resp.content_length() {
        pb.set_length(len);
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

    Ok(dest)
}
