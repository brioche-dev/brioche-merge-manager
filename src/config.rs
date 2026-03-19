use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub github_token: String,
    pub owner: String,
    pub repo: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let github_token = std::env::var("GITHUB_TOKEN")
            .context("GITHUB_TOKEN environment variable is required")?;

        let owner = std::env::var("GITHUB_OWNER")
            .unwrap_or_else(|_| "brioche-dev".to_string());

        let repo = std::env::var("GITHUB_REPO")
            .unwrap_or_else(|_| "brioche-packages".to_string());

        Ok(Self { github_token, owner, repo })
    }
}
