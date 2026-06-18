use anyhow::Result;

use crate::config::Config;
use crate::github::GithubClient;
use crate::github::types::Release;
use crate::installer::{self, InstallResult};
use crate::manifest::Manifest;
use crate::matcher::score::detect_arch;
use crate::output::{print_info, print_success, print_warning};
use crate::picker;
use crate::state::State;

/// How `resolve_and_install` should choose which release to install.
pub enum ReleaseSelection {
    /// Install this exact tag (version pinning). Resolved via `get_release_by_tag`.
    Tag(String),
    /// Install the newest non-draft release without prompting (used by `sync`).
    Latest,
    /// Show the interactive release picker (the default `ghr install` flow).
    InteractivePick,
}

fn filter_releases(releases: &mut Vec<Release>, include_prerelease: bool, config: &Config) {
    if !include_prerelease && !config.include_prereleases {
        releases.retain(|r| !r.prerelease && !r.draft);
    } else {
        releases.retain(|r| !r.draft);
    }
}

/// Shared install core for both `install` and `sync`: resolve a `Release` per `selection`,
/// pick the matching asset, download + install it, and upsert the result into `state`.
/// Does NOT save state or touch the manifest — callers own that so they can batch writes.
pub async fn resolve_and_install(
    client: &GithubClient,
    repo: &str,
    selection: ReleaseSelection,
    include_prerelease: bool,
    config: &Config,
    state: &mut State,
) -> Result<InstallResult> {
    let binary_name = repo.split('/').next_back().unwrap_or(repo);

    let release = match selection {
        ReleaseSelection::Tag(tag) => client.get_release_by_tag(repo, &tag).await?,
        ReleaseSelection::Latest => {
            let mut releases = client.list_releases(repo).await?;
            filter_releases(&mut releases, include_prerelease, config);
            releases
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no releases found for {repo}"))?
        }
        ReleaseSelection::InteractivePick => {
            let mut releases = client.list_releases(repo).await?;
            filter_releases(&mut releases, include_prerelease, config);
            if releases.is_empty() {
                anyhow::bail!("no releases found for {repo}");
            }

            let labels: Vec<String> = releases
                .iter()
                .map(|r| format!("{} ({})", r.tag_name, r.published_at.format("%Y-%m-%d")))
                .collect();
            let idx = dialoguer::FuzzySelect::new()
                .with_prompt("Pick a release")
                .items(&labels)
                .default(0)
                .interact()?;

            let release = releases.swap_remove(idx);
            println!("Selected: {} — {}", release.tag_name, release.html_url);
            release
        }
    };

    let user_arch = detect_arch();
    let asset = picker::select_asset(&release, &user_arch, None, repo, "Pick an asset")?;

    let pb = installer::download::make_progress_bar(None, asset.size, binary_name, None);
    let result = installer::install_asset(
        client.http_client(),
        repo,
        &release,
        &asset,
        binary_name,
        &config.install_dir,
        &release.assets,
        pb,
    )
    .await?;

    state.upsert(result.tool_entry.clone());
    Ok(result)
}

/// `ghr install <owner/repo> [-t <tag>]`. Pins to `tag` when given (and records the pin in
/// the manifest), otherwise shows the interactive release picker.
pub async fn cmd_install(
    repo: &str,
    tag: Option<String>,
    include_prerelease: bool,
    config: &Config,
) -> Result<()> {
    // Look for an already-managed tool with the same repo before doing any network I/O.
    let mut state = State::load()?;
    let binary_name = repo.split('/').next_back().unwrap_or(repo);
    let already_managed = state
        .get(binary_name)
        .is_some_and(|existing| existing.repo == repo);

    if already_managed {
        // Re-installing a managed tool only makes sense to move its pin, so require an
        // explicit tag. A bare `ghr install <repo>` must not silently reinstall — that's
        // what `ghr update` is for.
        let Some(new_tag) = tag.as_deref() else {
            let existing = state.get(binary_name).unwrap();
            anyhow::bail!(
                "'{binary_name}' is already managed by ghr ({}). \
                     Run `ghr update {binary_name}` to upgrade it, \
                     or pass `-t <tag>` to re-pin it to a specific release.",
                existing.installed_tag
            );
        };
        // Re-point in place: reinstall at the new tag and move the manifest pin below. Skip
        // the on-PATH adoption prompt — the binary it would find is ours.
        // WARN: For an adopted tool that lives in some other directory (e.g. /usr/local/bin),
        // re-pointing with -t would place the new binary in install_dir and leave the original behind.
        print_info(&format!("Re-pointing {binary_name} to {new_tag}."));
    } else if let Some(existing_path) = crate::find_on_path(binary_name) {
        // Not managed by ghr, but a binary with this name already exists on $PATH.
        print_warning(&format!(
            "'{binary_name}' is already installed at {}",
            existing_path.display()
        ));
        let proceed = dialoguer::Confirm::new()
            .with_prompt("Install anyway and let ghr manage it going forward?")
            .default(false)
            .interact()?;
        if !proceed {
            print_info("Installation cancelled.");
            return Ok(());
        }
    }

    let token = GithubClient::resolve_token(config.github_token.clone());
    let client = GithubClient::new(token)?;

    let selection = match tag.clone() {
        Some(t) => ReleaseSelection::Tag(t),
        None => ReleaseSelection::InteractivePick,
    };

    let result = resolve_and_install(
        &client,
        repo,
        selection,
        include_prerelease,
        config,
        &mut state,
    )
    .await?;
    state.save()?;

    // Keep the declarative manifest in sync. The tag (Some => pinned, None => clears any
    // existing pin) is recorded against the repo so `ghr sync` can replay it elsewhere.
    let mut manifest = Manifest::load()?;
    manifest.upsert(repo, tag);
    manifest.save()?;

    print_success(&format!(
        "Installed {} {} → {}",
        binary_name,
        result.tool_entry.installed_tag,
        result.installed_path.display()
    ));

    // Warn if install_dir is not on PATH
    if let Ok(path_var) = std::env::var("PATH") {
        let on_path = path_var
            .split(':')
            .any(|p| std::path::Path::new(p) == config.install_dir);
        if !on_path {
            print_warning(&format!(
                "{} is not on your PATH. Add it: export PATH=\"{}:$PATH\"",
                config.install_dir.display(),
                config.install_dir.display()
            ));
        }
    }

    Ok(())
}
