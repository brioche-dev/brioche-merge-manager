use anyhow::Result;
use tracing::debug;

use crate::config::Config;

use super::graphql::{enqueue_pull_request, enqueue_pull_requests_batch, fetch_all_prs_bulk};
use super::models::{FileDiff, MergeQueueEntry, PullRequest};
use super::rest::{build_pull_requests, fetch_pr_files};

pub struct GitHubClient {
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl GitHubClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            token: config.github_token.clone(),
            owner: config.owner.clone(),
            repo: config.repo.clone(),
        })
    }

    pub async fn fetch_managed_prs(
        &self,
        progress: tokio::sync::mpsc::UnboundedSender<(usize, usize)>,
    ) -> Result<Vec<PullRequest>> {
        debug!("fetch_managed_prs: starting");
        let raw_prs = fetch_all_prs_bulk(&self.token, &self.owner, &self.repo, &progress).await?;
        debug!(
            count = raw_prs.len(),
            "fetch_managed_prs: bulk fetch returned PRs"
        );
        let prs = build_pull_requests(raw_prs);
        debug!(count = prs.len(), "fetch_managed_prs: done");
        Ok(prs)
    }

    pub async fn enqueue_pr(&self, node_id: &str) -> Result<MergeQueueEntry> {
        debug!(%node_id, "enqueue_pr");
        enqueue_pull_request(&self.token, node_id).await
    }

    /// Enqueue multiple PRs in one batched GraphQL call.
    pub async fn enqueue_prs(
        &self,
        targets: &[(u64, String)],
    ) -> Vec<(u64, Result<MergeQueueEntry>)> {
        debug!(count = targets.len(), "enqueue_prs");
        match enqueue_pull_requests_batch(&self.token, targets).await {
            Ok(results) => results,
            Err(e) => {
                let msg = e.to_string();
                targets
                    .iter()
                    .map(|(n, _)| (*n, Err(anyhow::anyhow!("{msg}"))))
                    .collect()
            }
        }
    }

    pub async fn fetch_diff(&self, pr_number: u64) -> Result<Vec<FileDiff>> {
        debug!(pr = pr_number, "fetch_diff");
        fetch_pr_files(&self.token, &self.owner, &self.repo, pr_number).await
    }
}
