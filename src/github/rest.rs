use super::models::{FileDiff, FileStatus, PrStatus, PullRequest};
use anyhow::Result;
use tracing::debug;

/// Sort all PRs: FailedMerge first, ReadyToMerge second, InQueue last,
/// descending by number within each group.
/// Filtering is handled by the UI layer via `App::visible_prs()`.
pub fn build_pull_requests(mut prs: Vec<PullRequest>) -> Vec<PullRequest> {
    prs.sort_by(|a, b| {
        let order = |s: &PrStatus| match s {
            PrStatus::FailedMerge => 0,
            PrStatus::ReadyToMerge => 1,
            PrStatus::InQueue => 2,
        };
        order(&a.status)
            .cmp(&order(&b.status))
            .then(b.number.cmp(&a.number))
    });
    debug!(
        total = prs.len(),
        failed = prs
            .iter()
            .filter(|p| p.status == PrStatus::FailedMerge)
            .count(),
        ready = prs
            .iter()
            .filter(|p| p.status == PrStatus::ReadyToMerge)
            .count(),
        queued = prs.iter().filter(|p| p.status == PrStatus::InQueue).count(),
        "build_pull_requests: sorted",
    );
    prs
}

const GITHUB_REST_URL: &str = "https://api.github.com";

/// Fetch the list of files changed in a PR via the REST API.
/// Returns up to 100 files (GitHub's per_page max for this endpoint).
pub async fn fetch_pr_files(
    token: &str,
    owner: &str,
    repo: &str,
    pr_number: u64,
) -> Result<Vec<FileDiff>> {
    let client = reqwest::Client::new();
    let url =
        format!("{GITHUB_REST_URL}/repos/{owner}/{repo}/pulls/{pr_number}/files?per_page=100");

    let text = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "brioche-merge-manager/0.1")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let val: serde_json::Value = serde_json::from_str(&text)?;

    let files: Vec<FileDiff> = val
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|f| {
                    let filename = f["filename"].as_str().unwrap_or("").to_string();
                    let status = FileStatus::from(f["status"].as_str().unwrap_or(""));
                    let additions = f["additions"].as_u64().unwrap_or(0) as u32;
                    let deletions = f["deletions"].as_u64().unwrap_or(0) as u32;
                    let patch = f["patch"].as_str().map(|s| s.to_string());
                    FileDiff {
                        filename,
                        status,
                        additions,
                        deletions,
                        patch,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    debug!(pr = pr_number, count = files.len(), "fetch_pr_files: done");
    Ok(files)
}
