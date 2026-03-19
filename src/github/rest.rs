use super::models::{PrStatus, PullRequest};
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
