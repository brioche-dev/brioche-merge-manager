use anyhow::Result;
use tracing::debug;

use crate::config::Config;

use super::graphql::{dequeue_pull_request, enqueue_pull_request, fetch_all_prs_bulk};
use super::models::{MergeQueueEntry, PullRequest};
use super::rest::build_pull_requests;

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
        let raw_prs =
            fetch_all_prs_bulk(&self.token, &self.owner, &self.repo, &progress).await?;
        debug!(count = raw_prs.len(), "fetch_managed_prs: bulk fetch returned PRs");
        let prs = build_pull_requests(raw_prs);
        debug!(count = prs.len(), "fetch_managed_prs: done");
        Ok(prs)
    }

    pub async fn enqueue_pr(&self, node_id: &str) -> Result<MergeQueueEntry> {
        debug!(%node_id, "enqueue_pr");
        enqueue_pull_request(&self.token, node_id).await
    }

    pub async fn retry_pr(&self, node_id: &str, queue_entry_id: &str) -> Result<MergeQueueEntry> {
        debug!(%node_id, %queue_entry_id, "retry_pr");
        dequeue_pull_request(&self.token, queue_entry_id).await?;
        enqueue_pull_request(&self.token, node_id).await
    }
}
