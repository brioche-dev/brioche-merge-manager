use anyhow::{anyhow, Result};
use serde_json::json;
use tokio::task::JoinSet;
use tracing::{debug, trace};

use chrono::{DateTime, Utc};

use super::models::{
    CheckRollupState, MergeQueueEntry, MergeQueueState, MergeableState, PullRequest, QueueRemoval,
    QueueRemovalReason, ReviewDecision,
};

const GRAPHQL_URL: &str = "https://api.github.com/graphql";

// Retry up to 4 times (attempts 1–4) on transient 5xx / network errors.
// Delays: 1s, 2s, 4s (exponential backoff).
const MAX_ATTEMPTS: u32 = 4;

// ---------------------------------------------------------------------------
// Core HTTP helper
// ---------------------------------------------------------------------------

/// GraphQL POST with exponential-backoff retry on 5xx / network errors.
/// Takes a shared `reqwest::Client` so connections are pooled across requests.
async fn graphql_post(
    client: &reqwest::Client,
    token: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mut attempt = 0u32;

    loop {
        attempt += 1;

        let result = client
            .post(GRAPHQL_URL)
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "brioche-merge-manager/0.1")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(body)
            .send()
            .await;

        let resp = match result {
            Ok(r) => r,
            Err(e) => {
                if attempt >= MAX_ATTEMPTS {
                    return Err(anyhow!(
                        "GitHub GraphQL network error after {attempt} attempts: {e}"
                    ));
                }
                let delay = std::time::Duration::from_secs(1u64 << (attempt - 1));
                debug!(attempt, delay_s = delay.as_secs(), %e, "graphql_post: network error, retrying");
                tokio::time::sleep(delay).await;
                continue;
            }
        };

        let status = resp.status();
        let text = resp.text().await?;

        debug!(
            attempt,
            http = status.as_u16(),
            body_len = text.len(),
            "graphql_post: response"
        );
        if tracing::enabled!(tracing::Level::TRACE) {
            let snippet: String = text.chars().take(2000).collect();
            trace!(body = %snippet, "graphql_post: response body");
        }

        if status.is_server_error() {
            if attempt >= MAX_ATTEMPTS {
                let preview: String = text.chars().take(200).collect();
                return Err(anyhow!(
                    "GitHub GraphQL returned HTTP {} after {attempt} attempts.\nBody: {preview}",
                    status.as_u16()
                ));
            }
            let delay = std::time::Duration::from_secs(1u64 << (attempt - 1));
            debug!(
                attempt,
                http = status.as_u16(),
                delay_s = delay.as_secs(),
                "graphql_post: server error, retrying"
            );
            tokio::time::sleep(delay).await;
            continue;
        }

        if text.is_empty() {
            return Err(anyhow!(
                "GitHub GraphQL returned HTTP {} with an empty body. \
                 Check that your GITHUB_TOKEN is valid and has the `repo` scope.",
                status.as_u16()
            ));
        }

        let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            let preview: String = text.chars().take(300).collect();
            anyhow!(
                "GitHub GraphQL response is not JSON (HTTP {}): {e}\nBody preview: {preview}",
                status.as_u16()
            )
        })?;

        if let Some(errors) = val.get("errors") {
            let msg = format_graphql_errors(errors);
            debug!(%errors, "graphql_post: GraphQL errors");
            return Err(anyhow!("GitHub GraphQL error: {msg}"));
        }

        return Ok(val);
    }
}

// ---------------------------------------------------------------------------
// Bulk PR fetch — two-phase parallel strategy
// ---------------------------------------------------------------------------

/// Fetches all open PRs using a two-phase parallel strategy:
///
/// **Phase 1** — sequential, lightweight: collect all page-start cursors by
/// paging through `pullRequests` with *no* node fields (only `pageInfo` and
/// `totalCount`).  Each round-trip is tiny.
///
/// **Phase 2** — parallel: fire every full-data page request simultaneously
/// via `tokio::task::JoinSet`, using the cursors collected in phase 1.
pub async fn fetch_all_prs_bulk(
    token: &str,
    owner: &str,
    repo: &str,
    progress: &tokio::sync::mpsc::UnboundedSender<(usize, usize)>,
) -> Result<Vec<PullRequest>> {
    // One client shared across all requests — enables HTTP/1.1 keep-alive pooling.
    let client = reqwest::Client::new();

    // Phase 1: collect cursors
    let (start_cursors, total_count) = collect_page_cursors(&client, token, owner, repo).await?;

    debug!(
        pages = start_cursors.len(),
        total_count, "fetch_all_prs_bulk: fetching pages in parallel"
    );
    let _ = progress.send((0, total_count));

    if start_cursors.is_empty() {
        return Ok(Vec::new());
    }

    // Phase 2: parallel page fetches
    let mut set: JoinSet<Result<(usize, Vec<PullRequest>)>> = JoinSet::new();

    for (page_idx, cursor) in start_cursors.into_iter().enumerate() {
        let client = client.clone();
        let token = token.to_string();
        let owner = owner.to_string();
        let repo = repo.to_string();
        set.spawn(async move {
            fetch_pr_page(&client, &token, &owner, &repo, cursor.as_deref(), page_idx).await
        });
    }

    let mut pages: Vec<(usize, Vec<PullRequest>)> = Vec::new();
    let mut loaded = 0usize;

    while let Some(res) = set.join_next().await {
        let (page_idx, prs) = res.map_err(|e| anyhow!("task join error: {e}"))??;
        loaded += prs.len();
        let _ = progress.send((loaded, total_count));
        pages.push((page_idx, prs));
    }

    // Restore original PR order (pages may complete out of order)
    pages.sort_unstable_by_key(|(idx, _)| *idx);
    let all_prs = pages.into_iter().flat_map(|(_, prs)| prs).collect();

    debug!(total = loaded, "fetch_all_prs_bulk: done");
    Ok(all_prs)
}

/// Phase 1: page through the PR list fetching *only* pagination metadata.
/// Returns the list of cursors that start each page (first entry is `None`)
/// and the total PR count.
async fn collect_page_cursors(
    client: &reqwest::Client,
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<(Vec<Option<String>>, usize)> {
    let query = r#"
        query GetCursors($owner: String!, $repo: String!, $cursor: String) {
          repository(owner: $owner, name: $repo) {
            pullRequests(first: 50, after: $cursor, states: [OPEN]) {
              totalCount
              pageInfo { hasNextPage endCursor }
            }
          }
        }
    "#;

    // First page always starts with no cursor.
    let mut start_cursors: Vec<Option<String>> = vec![None];
    let mut cursor: Option<String> = None;
    let mut total_count = 0usize;

    loop {
        let body = json!({
            "query": query,
            "variables": { "owner": owner, "repo": repo, "cursor": cursor }
        });

        let response = graphql_post(client, token, &body).await?;
        let pr_page = &response["data"]["repository"]["pullRequests"];

        if let Some(n) = pr_page["totalCount"].as_u64() {
            total_count = n as usize;
        }

        let has_next = pr_page["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false);
        if !has_next {
            break;
        }

        let end_cursor = pr_page["pageInfo"]["endCursor"]
            .as_str()
            .map(|s| s.to_string());
        cursor = end_cursor.clone();
        start_cursors.push(end_cursor);
    }

    debug!(
        pages = start_cursors.len(),
        total_count, "collect_page_cursors: done"
    );
    Ok((start_cursors, total_count))
}

/// Phase 2: fetch one full-data page starting at `cursor`.
/// Returns `(page_idx, prs)` so the caller can reassemble in order.
async fn fetch_pr_page(
    client: &reqwest::Client,
    token: &str,
    owner: &str,
    repo: &str,
    cursor: Option<&str>,
    page_idx: usize,
) -> Result<(usize, Vec<PullRequest>)> {
    let query = r#"
        query GetPRPage($owner: String!, $repo: String!, $cursor: String) {
          repository(owner: $owner, name: $repo) {
            pullRequests(first: 50, after: $cursor, states: [OPEN]) {
              nodes {
                number
                id
                title
                url
                author { login }
                isDraft
                mergeable
                mergeStateStatus
                reviewDecision
                mergeQueueEntry { id state position }
                commits(last: 1) {
                  nodes { commit { statusCheckRollup { state } } }
                }
                timelineItems(last: 5, itemTypes: [REMOVED_FROM_MERGE_QUEUE_EVENT]) {
                  nodes {
                    ... on RemovedFromMergeQueueEvent {
                      createdAt
                      reason
                    }
                  }
                }
              }
            }
          }
        }
    "#;

    let body = json!({
        "query": query,
        "variables": { "owner": owner, "repo": repo, "cursor": cursor }
    });

    let response = graphql_post(client, token, &body).await?;
    let nodes = &response["data"]["repository"]["pullRequests"]["nodes"];

    let mut prs = Vec::new();
    if let Some(arr) = nodes.as_array() {
        debug!(
            page = page_idx,
            count = arr.len(),
            "fetch_pr_page: received nodes"
        );
        for node in arr {
            let pr = parse_pr_node(node);
            trace!(
                pr = pr.number,
                draft = pr.is_draft,
                merge_state = node["mergeStateStatus"].as_str().unwrap_or("null"),
                rollup = pr.check_rollup.as_ref().map(|r| r.label()).unwrap_or("none"),
                review = pr.review_decision.as_ref().map(|r| r.label()).unwrap_or("none"),
                queue_removal = pr.last_queue_removal.as_ref().map(|r| r.reason.label()).unwrap_or("none"),
                status = ?pr.status,
                "parsed PR",
            );
            prs.push(pr);
        }
    }

    Ok((page_idx, prs))
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_pr_node(node: &serde_json::Value) -> PullRequest {
    let number = node["number"].as_u64().unwrap_or(0);
    let node_id = node["id"].as_str().unwrap_or("").to_string();
    let title = node["title"].as_str().unwrap_or("").to_string();
    let author = node["author"]["login"].as_str().unwrap_or("").to_string();
    let html_url = node["url"].as_str().unwrap_or("").to_string();
    let is_draft = node["isDraft"].as_bool().unwrap_or(false);

    let mergeable_state = match node["mergeStateStatus"].as_str() {
        Some(s) => merge_state_status_to_state(s),
        None => match node["mergeable"].as_str().unwrap_or("UNKNOWN") {
            "MERGEABLE" => MergeableState::Clean,
            "CONFLICTING" => MergeableState::Dirty,
            _ => MergeableState::Unknown,
        },
    };

    let merge_queue = parse_merge_queue_entry(&node["mergeQueueEntry"]);
    let last_queue_removal = parse_queue_removal(&node["timelineItems"]);

    let check_rollup = node["commits"]["nodes"]
        .as_array()
        .and_then(|ns| ns.first())
        .and_then(|n| n["commit"]["statusCheckRollup"]["state"].as_str())
        .map(CheckRollupState::from_graphql);

    let review_decision = node["reviewDecision"]
        .as_str()
        .and_then(ReviewDecision::from_graphql);

    let status = PullRequest::compute_status(
        &mergeable_state,
        &merge_queue,
        is_draft,
        &last_queue_removal,
    );

    PullRequest {
        number,
        node_id,
        title,
        author,
        html_url,
        mergeable_state,
        merge_queue,
        check_rollup,
        review_decision,
        is_draft,
        status,
        last_queue_removal,
    }
}

/// Parse the most recent `REMOVED_FROM_MERGE_QUEUE_EVENT` for this PR.
/// No time limit — any removal on a still-open PR is relevant.
fn parse_queue_removal(timeline: &serde_json::Value) -> Option<QueueRemoval> {
    let nodes = timeline["nodes"].as_array()?;
    // Events are in chronological order; take the most recent (last).
    let event = nodes.iter().rev().find(|n| !n["createdAt"].is_null())?;
    let created_at = event["createdAt"]
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))?;
    let reason = QueueRemovalReason::from(event["reason"].as_str().unwrap_or(""));
    Some(QueueRemoval {
        at: created_at,
        reason,
    })
}

fn merge_state_status_to_state(s: &str) -> MergeableState {
    match s {
        "CLEAN" => MergeableState::Clean,
        "DIRTY" => MergeableState::Dirty,
        "BLOCKED" => MergeableState::Blocked,
        "BEHIND" => MergeableState::Behind,
        "UNSTABLE" => MergeableState::Unstable,
        _ => MergeableState::Unknown,
    }
}

fn parse_merge_queue_entry(val: &serde_json::Value) -> Option<MergeQueueEntry> {
    if val.is_null() {
        return None;
    }
    let id = val["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return None;
    }
    let state = MergeQueueState::from(val["state"].as_str().unwrap_or("QUEUED"));
    let position = val["position"].as_u64().unwrap_or(0) as u32;
    Some(MergeQueueEntry {
        id,
        state,
        position,
    })
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

/// Extract human-readable messages from a GraphQL `errors` array.
/// Falls back to the raw JSON if the structure is unexpected.
fn format_graphql_errors(errors: &serde_json::Value) -> String {
    if let Some(arr) = errors.as_array() {
        let messages: Vec<&str> = arr.iter().filter_map(|e| e["message"].as_str()).collect();
        if !messages.is_empty() {
            return messages.join("; ");
        }
    }
    errors.to_string()
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// Enqueue multiple PRs in a single GraphQL request using aliased mutations.
/// Returns a vec of `(pr_number, Result<MergeQueueEntry>)` — one entry per input.
pub async fn enqueue_pull_requests_batch(
    token: &str,
    targets: &[(u64, String)], // (pr_number, node_id)
) -> Result<Vec<(u64, Result<MergeQueueEntry>)>> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }

    // Build a mutation with one aliased field per target.
    let mut mutation = String::from("mutation BulkEnqueue {");
    for (i, (_, node_id)) in targets.iter().enumerate() {
        mutation.push_str(&format!(
            "\n  pr{i}: enqueuePullRequest(input: {{ pullRequestId: \"{node_id}\" }}) {{ mergeQueueEntry {{ id state position }} }}"
        ));
    }
    mutation.push_str("\n}");

    let body = json!({ "query": mutation });

    let client = reqwest::Client::new();
    let response = graphql_post(&client, token, &body).await?;
    let data = &response["data"];

    let mut results = Vec::with_capacity(targets.len());
    for (i, (pr_number, _)) in targets.iter().enumerate() {
        let alias = format!("pr{i}");
        let entry_val = &data[&alias]["mergeQueueEntry"];
        let id = entry_val["id"].as_str().filter(|s| !s.is_empty());
        let result = match id {
            Some(id) => {
                let state = MergeQueueState::from(entry_val["state"].as_str().unwrap_or("QUEUED"));
                let position = entry_val["position"].as_u64().unwrap_or(0) as u32;
                Ok(MergeQueueEntry {
                    id: id.to_string(),
                    state,
                    position,
                })
            }
            None => Err(anyhow!(
                "no mergeQueueEntry returned for PR #{pr_number} — already queued or ineligible"
            )),
        };
        results.push((*pr_number, result));
    }

    Ok(results)
}

pub async fn enqueue_pull_request(token: &str, pull_request_id: &str) -> Result<MergeQueueEntry> {
    let mutation = r#"
        mutation EnqueuePR($pullRequestId: ID!) {
          enqueuePullRequest(input: { pullRequestId: $pullRequestId }) {
            mergeQueueEntry { id state position }
          }
        }
    "#;

    let body = json!({
        "query": mutation,
        "variables": { "pullRequestId": pull_request_id }
    });

    let client = reqwest::Client::new();
    let response = graphql_post(&client, token, &body).await?;
    let entry_val = &response["data"]["enqueuePullRequest"]["mergeQueueEntry"];

    let id = entry_val["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("enqueuePullRequest returned no mergeQueueEntry — the PR may already be queued or ineligible"))?
        .to_string();

    let state = MergeQueueState::from(entry_val["state"].as_str().unwrap_or("QUEUED"));
    let position = entry_val["position"].as_u64().unwrap_or(0) as u32;

    Ok(MergeQueueEntry {
        id,
        state,
        position,
    })
}
