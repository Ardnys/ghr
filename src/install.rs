use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::{self, Config};
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

// TODO: Installation is getting complicated. It needs an abstraction

/// Shared install core for both `install` and `sync`: resolve a `Release` per `selection`,
/// pick the matching asset, download + install it, and upsert the result into `state`.
/// Does NOT save state or touch the manifest — callers own that so they can batch writes.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_and_install(
    client: &GithubClient,
    repo: &str,
    selection: ReleaseSelection,
    include_prerelease: bool,
    config: &Config,
    install_dir: &Path,
    alias: Option<&str>,
    state: &mut State,
) -> Result<InstallResult> {
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

    // `find_name` locates the binary inside the archive (repo-derived, unaffected by alias);
    // `install_name` is the filename + state key, overridden by `--alias` when given.
    let find_name = repo.split('/').next_back().unwrap_or(repo);
    let install_name = alias.unwrap_or(find_name);

    let pb = installer::download::make_progress_bar(None, asset.size, install_name, None);
    let result = installer::install_asset(
        client.http_client(),
        repo,
        &release,
        &asset,
        install_name,
        install_dir,
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
    alias: Option<String>,
    to: Option<PathBuf>,
    include_prerelease: bool,
    config: &Config,
) -> Result<()> {
    // Resolve the effective install directory: a `--to` override (with `~` expanded) wins,
    // otherwise the configured install_dir. The chosen dir is stored in the tool's
    // install_path, so future `ghr update`s reinstall here too — same as adopted tools.
    let install_dir = match &to {
        Some(p) => config::expand_tilde(p),
        None => config.install_dir.clone(),
    };

    // Look for an already-managed tool under its install name (the `--alias`, or the
    // repo-derived default) before doing any network I/O.
    let mut state = State::load()?;
    let install_name = match alias.as_deref() {
        Some(a) => a,
        None => repo.split('/').next_back().unwrap_or(repo),
    };
    let already_managed = state
        .get(install_name)
        .is_some_and(|existing| existing.repo == repo);

    if already_managed {
        // Re-installing a managed tool only makes sense to move its pin, so require an
        // explicit tag. A bare `ghr install <repo>` must not silently reinstall — that's
        // what `ghr update` is for.
        let Some(new_tag) = tag.as_deref() else {
            let existing = state.get(install_name).unwrap();
            anyhow::bail!(
                "'{install_name}' is already managed by ghr ({}). \
                     Run `ghr update {install_name}` to upgrade it, \
                     or pass `-t <tag>` to re-pin it to a specific release.",
                existing.installed_tag
            );
        };
        // Re-point in place: reinstall at the new tag and move the manifest pin below. Skip
        // the on-PATH adoption prompt — the binary it would find is ours.
        // WARN: For an adopted tool that lives in some other directory (e.g. /usr/local/bin),
        // re-pointing with -t would place the new binary in install_dir and leave the original behind.
        print_info(&format!("Re-pointing {install_name} to {new_tag}."));
    } else if let Some(existing_path) = crate::find_on_path(install_name) {
        // Not managed by ghr, but a binary with this name already exists on $PATH.
        print_warning(&format!(
            "'{install_name}' is already installed at {}",
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
        &install_dir,
        alias.as_deref(),
        &mut state,
    )
    .await?;
    state.save()?;

    print_success(&format!(
        "Installed {} {} → {}",
        install_name,
        result.tool_entry.installed_tag,
        result.installed_path.display()
    ));

    // Keep the declarative manifest in sync. The tag (Some => pinned, None => clears any
    // existing pin) and the alias are recorded against the repo so `ghr sync` can replay
    // both elsewhere.
    let mut manifest = Manifest::load()?;
    manifest.record(repo, tag, alias);
    manifest.save()?;

    // Warn if the install dir is not on PATH
    // BUG: it looks weird with "." as a path
    if let Ok(path_var) = std::env::var("PATH") {
        let on_path = path_var.split(':').any(|p| Path::new(p) == install_dir);
        if !on_path {
            print_warning(&format!(
                "{} is not on your PATH. Add it: export PATH=\"{}:$PATH\"",
                install_dir.display(),
                install_dir.display()
            ));
        }
    }

    Ok(())
}
