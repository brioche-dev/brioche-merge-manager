use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MergeableState {
    Clean,
    Dirty,
    Blocked,
    Behind,
    Unstable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MergeQueueState {
    Queued,
    Awaiting,
    Mergeable,
    Unmergeable,
    Locked,
}

impl From<&str> for MergeQueueState {
    fn from(s: &str) -> Self {
        match s {
            "QUEUED" => Self::Queued,
            "AWAITING_CHECKS" => Self::Awaiting,
            "MERGEABLE" => Self::Mergeable,
            "UNMERGEABLE" => Self::Unmergeable,
            "LOCKED" => Self::Locked,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueueEntry {
    pub id: String,
    pub state: MergeQueueState,
    pub position: u32,
}

/// Rolled-up result of all status checks on the head commit.
/// Maps from GraphQL `statusCheckRollup.state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CheckRollupState {
    Success,
    Failure,
    Pending,
    Error,
    /// No checks are configured for this repo/branch.
    Expected,
}

impl CheckRollupState {
    pub fn from_graphql(s: &str) -> Self {
        match s {
            "SUCCESS" => Self::Success,
            "FAILURE" => Self::Failure,
            "PENDING" => Self::Pending,
            "ERROR" => Self::Error,
            _ => Self::Expected,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            Self::Success => "✓",
            Self::Failure | Self::Error => "✗",
            Self::Pending => "…",
            Self::Expected => "—",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Success => Color::Green,
            Self::Failure | Self::Error => Color::Red,
            Self::Pending => Color::Yellow,
            Self::Expected => Color::DarkGray,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Pending => "pending",
            Self::Error => "error",
            Self::Expected => "no checks",
        }
    }
}

/// Review decision from GitHub's branch protection rules.
/// Maps from GraphQL `reviewDecision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

impl ReviewDecision {
    pub fn from_graphql(s: &str) -> Option<Self> {
        match s {
            "APPROVED" => Some(Self::Approved),
            "CHANGES_REQUESTED" => Some(Self::ChangesRequested),
            "REVIEW_REQUIRED" => Some(Self::ReviewRequired),
            _ => None,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            Self::Approved => "✓",
            Self::ChangesRequested => "✗",
            Self::ReviewRequired => "○",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Approved => Color::Green,
            Self::ChangesRequested => Color::Red,
            Self::ReviewRequired => Color::Yellow,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::ReviewRequired => "review required",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unknown,
}

impl From<&str> for FileStatus {
    fn from(s: &str) -> Self {
        match s {
            "added" => Self::Added,
            "modified" => Self::Modified,
            "deleted" => Self::Deleted,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            _ => Self::Unknown,
        }
    }
}

impl FileStatus {
    pub fn symbol(&self) -> &str {
        match self {
            Self::Added => "+",
            Self::Modified => "~",
            Self::Deleted => "-",
            Self::Renamed => "→",
            Self::Copied => "⎘",
            Self::Unknown => "?",
        }
    }

    pub fn color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Added => Color::Green,
            Self::Modified => Color::Yellow,
            Self::Deleted => Color::Red,
            Self::Renamed | Self::Copied => Color::Cyan,
            Self::Unknown => Color::Reset,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub filename: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    /// Raw unified diff patch, absent for binary files.
    pub patch: Option<String>,
}

/// Why a PR was removed from the merge queue.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueRemovalReason {
    /// Status checks failed — the most common retry case.
    FailedChecks,
    /// The branch has a merge conflict.
    MergeConflict,
    /// Rejected by a merge queue rule.
    RejectedByRule,
    /// Manually removed or queue cleared — not a failure.
    Other,
}

impl From<&str> for QueueRemovalReason {
    fn from(s: &str) -> Self {
        match s {
            "FAILED_CHECKS" => Self::FailedChecks,
            "MERGE_CONFLICT" => Self::MergeConflict,
            "REJECTED_BY_MERGE_QUEUE_RULE" => Self::RejectedByRule,
            _ => Self::Other,
        }
    }
}

impl QueueRemovalReason {
    pub fn label(&self) -> &str {
        match self {
            Self::FailedChecks => "failed status checks",
            Self::MergeConflict => "merge conflict",
            Self::RejectedByRule => "rejected by merge queue rule",
            Self::Other => "removed",
        }
    }

    /// Whether this reason should cause the PR to appear as FailedMerge.
    pub fn is_failure(&self) -> bool {
        !matches!(self, Self::Other)
    }
}

/// The most recent removal of this PR from the merge queue, if within 24 hours.
#[derive(Debug, Clone)]
pub struct QueueRemoval {
    pub at: chrono::DateTime<chrono::Utc>,
    pub reason: QueueRemovalReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrStatus {
    ReadyToMerge,
    FailedMerge,
    InQueue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub node_id: String,
    pub title: String,
    pub author: String,
    pub html_url: String,
    pub mergeable_state: MergeableState,
    pub merge_queue: Option<MergeQueueEntry>,
    pub check_rollup: Option<CheckRollupState>,
    pub review_decision: Option<ReviewDecision>,
    pub is_draft: bool,
    pub status: PrStatus,
    /// Most recent removal from the merge queue within the past 24 hours, if any.
    #[serde(skip)]
    pub last_queue_removal: Option<QueueRemoval>,
}

impl PullRequest {
    pub fn compute_status(
        mergeable_state: &MergeableState,
        merge_queue: &Option<MergeQueueEntry>,
        _is_draft: bool,
        last_queue_removal: &Option<QueueRemoval>,
    ) -> PrStatus {
        if let Some(entry) = merge_queue {
            // PR is currently in the queue.
            match entry.state {
                MergeQueueState::Unmergeable => PrStatus::FailedMerge,
                MergeQueueState::Queued
                | MergeQueueState::Awaiting
                | MergeQueueState::Mergeable
                | MergeQueueState::Locked => PrStatus::InQueue,
            }
        } else {
            // PR is not in the queue — check if it was recently ejected.
            if let Some(removal) = last_queue_removal {
                if removal.reason.is_failure() {
                    return PrStatus::FailedMerge;
                }
            }
            match mergeable_state {
                MergeableState::Clean => PrStatus::ReadyToMerge,
                MergeableState::Blocked => PrStatus::FailedMerge,
                _ => PrStatus::ReadyToMerge,
            }
        }
    }
}
