use anyhow::Result;

use crate::config::Config;
use crate::github::GithubClient;
use crate::install::{ReleaseSelection, resolve_and_install};
use crate::manifest::Manifest;
use crate::output::{print_info, print_success, print_warning};
use crate::state::State;

/// `ghr sync`: install every tool in the manifest that isn't already in local state.
/// Pinned entries install their exact tag; the rest install the latest release. Tools
/// already present are left untouched (re-versioning is `update`'s job, not `sync`'s).
pub async fn cmd_sync(config: &Config) -> Result<()> {
    let manifest = Manifest::load()?;
    let mut state = State::load()?;

    if manifest.tools.is_empty() {
        print_info(&format!(
            "Manifest is empty ({}). Install a tool or add entries to it first.",
            Manifest::manifest_path().display()
        ));
        return Ok(());
    }

    let token = GithubClient::resolve_token(config.github_token.clone());
    let client = GithubClient::new(token)?;

    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for entry in manifest.iter() {
        let repo = &entry.repo;

        if state.contains_repo(repo) {
            skipped += 1;
            continue;
        }

        let selection = match &entry.tag {
            Some(tag) => {
                print_info(&format!("Installing {repo} (pinned {tag})..."));
                ReleaseSelection::Tag(tag.clone())
            }
            None => {
                print_info(&format!("Installing {repo} (latest)..."));
                ReleaseSelection::Latest
            }
        };

        match resolve_and_install(
            &client,
            repo,
            selection,
            false,
            config,
            &config.install_dir,
            entry.alias.as_deref(),
            &mut state,
        )
        .await
        {
            Ok(result) => {
                installed += 1;
                print_success(&format!(
                    "{} {} → {}",
                    result.tool_entry.binary_name,
                    result.tool_entry.installed_tag,
                    result.installed_path.display()
                ));
            }
            Err(e) => {
                failed += 1;
                print_warning(&format!("Failed to install {repo}: {e:#}"));
            }
        }
    }

    if installed > 0 {
        state.save()?;
    }

    println!();
    print_info(&format!(
        "Sync complete: {installed} installed, {skipped} already present, {failed} failed."
    ));

    Ok(())
}
