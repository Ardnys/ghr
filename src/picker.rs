use anyhow::Result;

use crate::github::types::{Asset, Release};
use crate::matcher::{MatchOutput, match_asset};
use crate::output::print_info;

/// Resolve a release to a single concrete asset for the current arch.
///
/// Auto-selects when the matcher is confident; otherwise falls back to an interactive
/// picker. `pattern` is the tool's stored `asset_pattern` for updates, or `None` for a
/// fresh install. This is the single selection path shared by install and both update flows.
pub fn select_asset(
    release: &Release,
    user_arch: &str,
    pattern: Option<&str>,
    repo: &str,
    prompt: &str,
) -> Result<Asset> {
    let match_output = match_asset(
        release.assets.clone(),
        user_arch,
        pattern,
        repo,
        &release.tag_name,
    )?;

    let asset = match match_output {
        MatchOutput::AutoSelected(s) => {
            print_info(&format!(
                "Auto-selected asset: {} (arch match: {}, {})",
                s.asset.name, s.score.arch_match, user_arch
            ));
            s.asset
        }
        MatchOutput::NeedsInteraction(candidates) => {
            let names: Vec<String> = candidates.iter().map(|c| c.asset.name.clone()).collect();
            let idx = dialoguer::Select::new()
                .with_prompt(prompt)
                .items(&names)
                .default(0)
                .interact()?;
            candidates.into_iter().nth(idx).unwrap().asset
        }
    };

    Ok(asset)
}
