mod app;
mod config;
mod event;
mod github;
mod tui;
mod ui;

use std::sync::Arc;

use anyhow::Result;
use tracing::debug;

use app::App;
use config::Config;
use github::GitHubClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env before reading any env vars
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = Config::from_env()?;
    debug!(owner = %config.owner, repo = %config.repo, "starting");

    let open_diff = std::env::args().any(|a| a == "--diff");

    let github = Arc::new(GitHubClient::new(&config)?);
    let app = App::new(config, github, open_diff);

    tui::run(app).await?;

    debug!("exiting cleanly");
    Ok(())
}

/// Initialise the tracing subscriber.
///
/// Set `DEBUG_LOG=/path/to/file` to enable logging to a file (the TUI owns
/// the terminal so stdout/stderr cannot be used at runtime).
/// Use `RUST_LOG` to control the filter (e.g. `RUST_LOG=debug`); defaults to
/// `debug` when `DEBUG_LOG` is set.
fn init_tracing() {
    let Some(path) = std::env::var("DEBUG_LOG").ok().filter(|s| !s.is_empty()) else {
        return;
    };

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            use tracing_subscriber::{fmt, EnvFilter};
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
            fmt()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_env_filter(filter)
                .init();
            debug!("tracing initialised → {path}");
        }
        Err(e) => eprintln!("DEBUG_LOG: could not open {path}: {e}"),
    }
}
