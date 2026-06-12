pub mod binary;
pub mod checksum;
pub mod download;
pub mod extract;

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::error::GhrError;
use crate::github::types::{Asset, Release};
use crate::matcher::pattern::asset_to_pattern;
use crate::output::{print_info, print_success, print_warning};
use crate::state::ToolEntry;
use extract::{BinarySearchResult, find_binary};

/// RAII guard that removes temp files on drop.
struct InstallGuard {
    temp_files: Vec<PathBuf>,
    temp_dirs: Vec<PathBuf>,
}

impl InstallGuard {
    fn new() -> Self {
        Self {
            temp_files: vec![],
            temp_dirs: vec![],
        }
    }

    fn track_file(&mut self, path: PathBuf) {
        self.temp_files.push(path);
    }

    fn track_dir(&mut self, path: PathBuf) {
        self.temp_dirs.push(path);
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        for f in &self.temp_files {
            let _ = std::fs::remove_file(f);
        }
        for d in &self.temp_dirs {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

pub struct InstallResult {
    pub tool_entry: ToolEntry,
    pub installed_path: PathBuf,
}

/// Full install pipeline: download → verify → extract → locate binary → install.
/// `install_dir` is where the final binary is placed; callers should pass
/// `entry.install_path.parent()` for updates so adopted tools stay in their original location.
pub async fn install_asset(
    client: &reqwest::Client,
    repo: &str,
    release: &Release,
    asset: &Asset,
    binary_name: &str,
    install_dir: &std::path::Path,
    all_assets: &[Asset],
) -> Result<InstallResult> {
    let mut guard = InstallGuard::new();

    // Step 1: Download to cache
    print_info(&format!(
        "Downloading {} ({} bytes)...",
        asset.name, asset.size
    ));
    let asset_path = download::download_to_cache(client, &asset.browser_download_url, &asset.name)
        .await
        .with_context(|| format!("failed to download {}", asset.name))?;
    guard.track_file(asset_path.clone());

    // Step 2: Checksum verification
    let installed_sha256 =
        if let Some(chk_asset) = checksum::find_checksum_asset(&asset.name, all_assets) {
            print_info("Verifying checksum...");
            match checksum::verify_checksum(
                client,
                &asset_path,
                &asset.name,
                &chk_asset.browser_download_url,
            )
            .await
            {
                Ok(hash) => {
                    print_success("Checksum verified.");
                    Some(hash)
                }
                Err(e) => {
                    // Guard will clean up asset_path
                    return Err(e);
                }
            }
        } else {
            print_warning("No checksum file found — skipping verification.");
            // Compute local hash anyway for state tracking
            checksum::sha256_file(&asset_path).ok()
        };

    // Step 3: Extract archive (or treat as raw binary)
    // Detect by filename extension first; fall back to content_type for assets with no extension.
    let asset_lower = asset.name.to_lowercase();
    let ct = asset.content_type.to_lowercase();
    let is_archive = asset_lower.ends_with(".tar.gz")
        || asset_lower.ends_with(".tgz")
        || asset_lower.ends_with(".tar.xz")
        || asset_lower.ends_with(".tar.bz2")
        || asset_lower.ends_with(".zip")
        || ct.contains("gzip")
        || ct.contains("x-tar")
        || ct.contains("x-xz")
        || ct.contains("x-bzip2")
        || ct == "application/zip";

    let binary_src = if is_archive {
        let extract_dir = download::cache_dir().join(format!("{}-extract", asset.name));
        guard.track_dir(extract_dir.clone());

        print_info("Extracting archive...");
        extract::extract_archive(&asset_path, &extract_dir)?;

        // Locate binary inside the extracted tree
        match find_binary(&extract_dir, binary_name)? {
            BinarySearchResult::Found(p) => p,
            BinarySearchResult::Multiple(candidates) => {
                let names: Vec<String> =
                    candidates.iter().map(|p| p.display().to_string()).collect();
                let selection = dialoguer::Select::new()
                    .with_prompt("Multiple binaries found — pick one")
                    .items(&names)
                    .default(0)
                    .interact()
                    .context("failed to show binary picker")?;
                candidates.into_iter().nth(selection).unwrap()
            }
            BinarySearchResult::NotFound => {
                return Err(GhrError::BinaryNotFoundInArchive.into());
            }
        }
    } else {
        asset_path.clone()
    };

    // Step 4+5: chmod + atomic install
    let installed_path = binary::atomic_install(&binary_src, install_dir, binary_name)?;

    // Step 6: Build ToolEntry
    let asset_pattern = asset_to_pattern(&asset.name, &release.tag_name);

    let tool_entry = ToolEntry {
        repo: repo.to_string(),
        installed_tag: release.tag_name.clone(),
        install_path: installed_path.clone(),
        binary_name: binary_name.to_string(),
        asset_pattern,
        installed_sha256,
        etag: None,
        last_checked: Some(Utc::now()),
        published_at: Some(release.published_at),
    };

    // Guard drops here — temp files are removed (guard owns asset_path clone, not installed_path)
    Ok(InstallResult {
        tool_entry,
        installed_path,
    })
}
