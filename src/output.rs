use console::style;

use crate::error::GhrError;
// TODO: proper logging alongside / instead of these

pub fn print_error(err: &anyhow::Error) {
    if let Some(ghr_err) = err.downcast_ref::<GhrError>() {
        match ghr_err {
            GhrError::RateLimitExceeded { .. } => {
                eprintln!("{} {}", style("error:").red().bold(), ghr_err);
                eprintln!(
                    "  {} Set GITHUB_TOKEN or add github_token to ~/.config/ghr/config.toml to increase the rate limit.",
                    style("hint:").yellow()
                );
            }
            GhrError::NoCompatibleAssets { .. } => {
                eprintln!("{} {}", style("error:").red().bold(), ghr_err);
                eprintln!(
                    "  {} Try `ghr install` with `--prerelease` or check the release page manually.",
                    style("hint:").yellow()
                );
            }
            GhrError::ChecksumMismatch { .. } => {
                eprintln!("{} {}", style("error:").red().bold(), ghr_err);
                eprintln!(
                    "  {} Downloaded file has been removed. The release asset may be corrupted.",
                    style("hint:").yellow()
                );
            }
            _ => {
                eprintln!("{} {}", style("error:").red().bold(), ghr_err);
            }
        }
    } else {
        eprintln!("{} {:#}", style("error:").red().bold(), err);
    }
}

pub fn print_success(msg: &str) {
    println!("{} {}", style("✓").green().bold(), msg);
}

pub fn print_warning(msg: &str) {
    eprintln!("{} {}", style("warning:").yellow().bold(), msg);
}

pub fn print_info(msg: &str) {
    println!("{} {}", style("info:").cyan().bold(), msg);
}
