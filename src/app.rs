use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;
use crate::event::Event;
use crate::github::models::{PrStatus, PullRequest};
use crate::github::GitHubClient;

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// ReadyToMerge + FailedMerge (excludes InQueue / drafts)
    Active,
    Ready,
    Failed,
    Queued,
}

impl Filter {
    pub const ALL: &'static [Filter] = &[
        Filter::Active,
        Filter::Ready,
        Filter::Failed,
        Filter::Queued,
    ];

    pub fn label(&self) -> &str {
        match self {
            Self::Active => "Active",
            Self::Ready => "Ready",
            Self::Failed => "Failed",
            Self::Queued => "Queued",
        }
    }

    pub fn matches(&self, pr: &PullRequest) -> bool {
        match self {
            Self::Active => {
                pr.status == PrStatus::ReadyToMerge || pr.status == PrStatus::FailedMerge
            }
            Self::Ready => pr.status == PrStatus::ReadyToMerge,
            Self::Failed => pr.status == PrStatus::FailedMerge,
            Self::Queued => pr.status == PrStatus::InQueue,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Active => Self::Ready,
            Self::Ready => Self::Failed,
            Self::Failed => Self::Queued,
            Self::Queued => Self::Active,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Active => Self::Queued,
            Self::Ready => Self::Active,
            Self::Failed => Self::Ready,
            Self::Queued => Self::Failed,
        }
    }
}

// ---------------------------------------------------------------------------
// LoadState / Action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LoadState {
    Idle,
    Loading,
    Error(String),
}

#[derive(Debug)]
pub enum Action {
    Tick,
    Refresh,
    NavigateUp,
    NavigateDown,
    CycleFilterNext,
    CycleFilterPrev,
    QueueSelected,
    RetrySelected,
    OpenInBrowser,
    Quit,
    DataLoaded(Vec<PullRequest>),
    LoadError(String),
    LoadProgress(usize, usize),
    StatusMessage(String),
    NavigatePageUp,
    NavigatePageDown,
    NavigateHome,
    NavigateEnd,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct App {
    pub config: Config,
    pub github: Arc<GitHubClient>,
    /// All PRs returned by the API (unfiltered, sorted).
    pub prs: Vec<PullRequest>,
    /// Index into the *filtered* visible list.
    pub selected: usize,
    pub list_state: ListState,
    pub load_state: LoadState,
    pub load_progress: Option<(usize, usize)>,
    /// Height of the visible list area in rows (updated each render frame).
    pub list_height: usize,
    pub active_filter: Filter,
    pub status_msg: Option<(String, Instant)>,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: Config, github: Arc<GitHubClient>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            config,
            github,
            prs: Vec::new(),
            selected: 0,
            list_state,
            load_state: LoadState::Idle,
            load_progress: None,
            list_height: 10,
            active_filter: Filter::Active,
            status_msg: None,
            should_quit: false,
        }
    }

    /// PRs visible under the current filter.
    pub fn visible_prs(&self) -> Vec<&PullRequest> {
        self.prs
            .iter()
            .filter(|pr| self.active_filter.matches(pr))
            .collect()
    }

    /// Count of PRs matching a given filter (for tab labels).
    pub fn count_for(&self, filter: &Filter) -> usize {
        self.prs.iter().filter(|pr| filter.matches(pr)).count()
    }

    /// Currently selected PR in the visible list.
    pub fn selected_pr(&self) -> Option<&PullRequest> {
        self.visible_prs().into_iter().nth(self.selected)
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_prs().len();
        if count == 0 {
            self.selected = 0;
            self.list_state.select(None);
        } else {
            self.selected = self.selected.min(count - 1);
            self.list_state.select(Some(self.selected));
        }
    }

    pub fn handle_event(&self, event: Event) -> Option<Action> {
        match event {
            Event::Tick => Some(Action::Tick),
            Event::Key(code, modifiers) => {
                if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                    return Some(Action::Quit);
                }
                match code {
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::NavigateUp),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::NavigateDown),
                    KeyCode::PageUp => Some(Action::NavigatePageUp),
                    KeyCode::PageDown => Some(Action::NavigatePageDown),
                    KeyCode::Home => Some(Action::NavigateHome),
                    KeyCode::End => Some(Action::NavigateEnd),
                    KeyCode::Tab => Some(Action::CycleFilterNext),
                    KeyCode::BackTab => Some(Action::CycleFilterPrev),
                    KeyCode::Char('q') => Some(Action::QueueSelected),
                    KeyCode::Char('r') => Some(Action::RetrySelected),
                    KeyCode::Char('R') => Some(Action::Refresh),
                    KeyCode::Char('o') => Some(Action::OpenInBrowser),
                    _ => None,
                }
            }
        }
    }

    pub async fn update(
        &mut self,
        action: Action,
        action_tx: &UnboundedSender<Action>,
    ) -> Result<()> {
        match action {
            Action::Tick => {
                if let Some((_, ts)) = &self.status_msg {
                    if ts.elapsed().as_secs() >= 3 {
                        self.status_msg = None;
                    }
                }
            }

            Action::Refresh => {
                self.load_state = LoadState::Loading;
                self.load_progress = None;
                let github = Arc::clone(&self.github);
                let tx = action_tx.clone();
                let (prog_tx, mut prog_rx) =
                    tokio::sync::mpsc::unbounded_channel::<(usize, usize)>();
                // Forward progress events to the action channel
                let fwd_tx = tx.clone();
                tokio::spawn(async move {
                    while let Some((loaded, total)) = prog_rx.recv().await {
                        let _ = fwd_tx.send(Action::LoadProgress(loaded, total));
                    }
                });
                tokio::spawn(async move {
                    match github.fetch_managed_prs(prog_tx).await {
                        Ok(prs) => {
                            let _ = tx.send(Action::DataLoaded(prs));
                        }
                        Err(e) => {
                            let _ = tx.send(Action::LoadError(e.to_string()));
                        }
                    }
                });
            }

            Action::NavigateUp => {
                if !self.visible_prs().is_empty() {
                    self.selected = self.selected.saturating_sub(1);
                    self.list_state.select(Some(self.selected));
                }
            }

            Action::NavigateDown => {
                let count = self.visible_prs().len();
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                    self.list_state.select(Some(self.selected));
                }
            }

            Action::NavigatePageUp => {
                let page = self.list_height.max(1);
                self.selected = self.selected.saturating_sub(page);
                self.list_state.select(Some(self.selected));
            }

            Action::NavigatePageDown => {
                let count = self.visible_prs().len();
                if count > 0 {
                    let page = self.list_height.max(1);
                    self.selected = (self.selected + page).min(count - 1);
                    self.list_state.select(Some(self.selected));
                }
            }

            Action::NavigateHome => {
                self.selected = 0;
                self.list_state.select(Some(0));
            }

            Action::NavigateEnd => {
                let count = self.visible_prs().len();
                if count > 0 {
                    self.selected = count - 1;
                    self.list_state.select(Some(self.selected));
                }
            }

            Action::CycleFilterNext => {
                self.active_filter = self.active_filter.next();
                self.selected = 0;
                self.clamp_selection();
            }

            Action::CycleFilterPrev => {
                self.active_filter = self.active_filter.prev();
                self.selected = 0;
                self.clamp_selection();
            }

            Action::QueueSelected => {
                if let Some(pr) = self.selected_pr() {
                    if pr.status == PrStatus::ReadyToMerge {
                        let github = Arc::clone(&self.github);
                        let node_id = pr.node_id.clone();
                        let pr_number = pr.number;
                        let tx = action_tx.clone();
                        tokio::spawn(async move {
                            match github.enqueue_pr(&node_id).await {
                                Ok(_) => {
                                    let _ = tx.send(Action::StatusMessage(format!(
                                        "PR #{pr_number} added to merge queue"
                                    )));
                                    let _ = tx.send(Action::Refresh);
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::StatusMessage(format!(
                                        "Error queuing PR #{pr_number}: {e}"
                                    )));
                                }
                            }
                        });
                    }
                }
            }

            Action::RetrySelected => {
                if let Some(pr) = self.selected_pr() {
                    if pr.status == PrStatus::FailedMerge {
                        let github = Arc::clone(&self.github);
                        let node_id = pr.node_id.clone();
                        let queue_entry_id = pr
                            .merge_queue
                            .as_ref()
                            .map(|e| e.id.clone())
                            .unwrap_or_default();
                        let pr_number = pr.number;
                        let tx = action_tx.clone();
                        tokio::spawn(async move {
                            match github.retry_pr(&node_id, &queue_entry_id).await {
                                Ok(_) => {
                                    let _ = tx.send(Action::StatusMessage(format!(
                                        "PR #{pr_number} re-queued for merge"
                                    )));
                                    let _ = tx.send(Action::Refresh);
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::StatusMessage(format!(
                                        "Error retrying PR #{pr_number}: {e}"
                                    )));
                                }
                            }
                        });
                    }
                }
            }

            Action::OpenInBrowser => {
                if let Some(pr) = self.selected_pr() {
                    let url = pr.html_url.clone();
                    let pr_number = pr.number;
                    let tx = action_tx.clone();
                    tokio::spawn(async move {
                        match open_url(&url) {
                            Ok(_) => {
                                let _ = tx.send(Action::StatusMessage(format!(
                                    "Opened PR #{pr_number} in browser"
                                )));
                            }
                            Err(e) => {
                                let _ = tx.send(Action::StatusMessage(format!(
                                    "Failed to open browser: {e}"
                                )));
                            }
                        }
                    });
                }
            }

            Action::Quit => {
                self.should_quit = true;
            }

            Action::LoadProgress(loaded, total) => {
                self.load_progress = Some((loaded, total));
            }

            Action::DataLoaded(prs) => {
                self.prs = prs;
                self.load_state = LoadState::Idle;
                self.load_progress = None;
                self.clamp_selection();
            }

            Action::LoadError(msg) => {
                self.load_state = LoadState::Error(msg);
                self.load_progress = None;
            }

            Action::StatusMessage(msg) => {
                self.status_msg = Some((msg, Instant::now()));
            }
        }

        Ok(())
    }
}

fn open_url(url: &str) -> Result<()> {
    webbrowser::open(url).map_err(|e| anyhow::anyhow!("{e}"))
}
