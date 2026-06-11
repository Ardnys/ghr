use anyhow::Result;

use crate::config::Config;
use crate::github::api::{ConditionalResult, GithubClient};
use crate::matcher::{match_asset, MatchOutput};
use crate::output::{print_info, print_success, print_warning};
use crate::state::State;

pub async fn cmd_update(name: Option<String>, all: bool, config: &Config) -> Result<()> {
    let mut state = State::load()?;

    if state.tools.is_empty() {
        print_info("No tools managed by ghr. Run `ghr install <owner/repo>` to get started.");
        return Ok(());
    }

    let tools_to_update: Vec<String> = if all {
        state.tools.keys().cloned().collect()
    } else if let Some(ref n) = name {
        if !state.tools.contains_key(n.as_str()) {
            return Err(crate::error::GhrError::UnknownTool { name: n.clone() }.into());
        }
        vec![n.clone()]
    } else {
        anyhow::bail!("specify a tool name or pass --all");
    };

    let token = GithubClient::resolve_token(config.github_token.clone());
    let client = GithubClient::new(token)?;

    for tool_name in &tools_to_update {
        let entry = state.tools.get(tool_name).unwrap().clone();
        print_info(&format!("Checking {} ({})...", tool_name, entry.repo));

        let result = client
            .get_latest_release(&entry.repo, entry.etag.as_deref())
            .await;

        match result {
            Ok(ConditionalResult::NotModified) => {
                print_success(&format!("{tool_name} is already up to date (304 Not Modified)."));
                if let Some(e) = state.tools.get_mut(tool_name) {
                    e.last_checked = Some(chrono::Utc::now());
                }
                state.save()?;
            }
            Ok(ConditionalResult::Changed(resp)) => {
                let release = resp.data;

                // Update etag and last_checked regardless of whether we install
                if let Some(e) = state.tools.get_mut(tool_name) {
                    e.etag = resp.etag;
                    e.last_checked = Some(chrono::Utc::now());
                }

                let is_newer = match entry.published_at {
                    Some(prev) => release.published_at > prev,
                    None => true,
                };

                if !is_newer {
                    print_success(&format!("{tool_name} is already up to date ({}).", entry.installed_tag));
                    state.save()?;
                    continue;
                }

                println!("  Update available: {} → {}", entry.installed_tag, release.tag_name);

                let user_arch = crate::matcher::score::detect_arch();
                let all_assets = release.assets.clone();

                let match_output = match_asset(
                    all_assets.clone(),
                    &user_arch,
                    Some(&entry.asset_pattern),
                    &entry.repo,
                    &release.tag_name,
                )?;

                let selected = match match_output {
                    MatchOutput::AutoSelected(s) => {
                        print_info(&format!("Auto-selected asset: {}", s.asset.name));
                        s
                    }
                    MatchOutput::NeedsInteraction(candidates) => {
                        let names: Vec<String> =
                            candidates.iter().map(|c| c.asset.name.clone()).collect();
                        let idx = dialoguer::Select::new()
                            .with_prompt("Pick an asset")
                            .items(&names)
                            .default(0)
                            .interact()?;
                        candidates.into_iter().nth(idx).unwrap()
                    }
                };

                // Respect the tool's original install location so adopted tools
                // (which may live outside config.install_dir) are updated in place.
                let install_dir = entry
                    .install_path
                    .parent()
                    .unwrap_or(&config.install_dir);

                let result = crate::installer::install_asset(
                    client.http_client(),
                    &entry.repo,
                    &release,
                    &selected.asset,
                    &entry.binary_name,
                    install_dir,
                    &all_assets,
                )
                .await?;

                state.tools.insert(tool_name.clone(), result.tool_entry);
                state.save()?;
                print_success(&format!(
                    "{tool_name} updated to {} → {}",
                    entry.installed_tag, release.tag_name
                ));
            }
            Err(e) => {
                print_warning(&format!("Failed to check {tool_name}: {e:#}"));
            }
        }
    }

    Ok(())
}

pub async fn cmd_check(json: bool, config: &Config) -> Result<()> {
    let mut state = State::load()?;

    if state.tools.is_empty() {
        if json {
            println!("[]");
        } else {
            print_info("No tools managed by ghr.");
        }
        return Ok(());
    }

    let token = GithubClient::resolve_token(config.github_token.clone());
    let client = GithubClient::new(token)?;

    #[derive(serde::Serialize)]
    struct CheckResult {
        name: String,
        installed_tag: String,
        latest_tag: Option<String>,
        update_available: bool,
    }

    let mut results: Vec<CheckResult> = Vec::new();
    let mut any_updates = false;

    for (name, entry) in &mut state.tools {
        let result = client
            .get_latest_release(&entry.repo, entry.etag.as_deref())
            .await;

        entry.last_checked = Some(chrono::Utc::now());

        match result {
            Ok(ConditionalResult::NotModified) => {
                results.push(CheckResult {
                    name: name.to_string(),
                    installed_tag: entry.installed_tag.clone(),
                    latest_tag: None,
                    update_available: false,
                });
            }
            Ok(ConditionalResult::Changed(resp)) => {
                let release = resp.data;
                entry.etag = resp.etag;

                let update_available = match entry.published_at {
                    Some(prev) => release.published_at > prev,
                    None => true,
                };

                if update_available {
                    any_updates = true;
                }

                results.push(CheckResult {
                    name: name.clone(),
                    installed_tag: entry.installed_tag.clone(),
                    latest_tag: Some(release.tag_name),
                    update_available,
                });
            }
            Err(e) => {
                print_warning(&format!("Failed to check {name}: {e:#}"));
            }
        }
    }

    state.save()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        for r in &results {
            if r.update_available {
                println!(
                    "  {} {} → {} (update available)",
                    console::style(&r.name).yellow().bold(),
                    r.installed_tag,
                    r.latest_tag.as_deref().unwrap_or("?")
                );
            } else {
                println!(
                    "  {} {} (up to date)",
                    console::style(&r.name).green().bold(),
                    r.installed_tag
                );
            }
        }

        if any_updates {
            println!();
            println!(
                "{}",
                console::style("Updates available — run `ghr update --all`").yellow()
            );
        }
    }

    // Exit 1 if updates available (useful for scripting)
    if any_updates {
        std::process::exit(1);
    }

    Ok(())
}
