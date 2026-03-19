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
}

impl PullRequest {
    pub fn compute_status(
        mergeable_state: &MergeableState,
        merge_queue: &Option<MergeQueueEntry>,
        is_draft: bool,
    ) -> PrStatus {
        // Drafts are never ready — exclude them the same way InQueue PRs are
        if is_draft {
            return PrStatus::InQueue;
        }

        if let Some(entry) = merge_queue {
            match entry.state {
                MergeQueueState::Unmergeable => PrStatus::FailedMerge,
                MergeQueueState::Queued
                | MergeQueueState::Awaiting
                | MergeQueueState::Mergeable
                | MergeQueueState::Locked => PrStatus::InQueue,
            }
        } else {
            match mergeable_state {
                MergeableState::Clean => PrStatus::ReadyToMerge,
                MergeableState::Blocked => PrStatus::FailedMerge,
                _ => PrStatus::ReadyToMerge,
            }
        }
    }
}
