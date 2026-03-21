use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{layout::Rect, widgets::ListState};
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;
use crate::event::Event;
use crate::github::models::{FileDiff, MergeQueueEntry, PrStatus, PullRequest};
use crate::github::GitHubClient;

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// ReadyToMerge + removed-from-queue (excludes InQueue / drafts)
    Active,
    Ready,
    Removed,
    Queued,
}

impl Filter {
    pub const ALL: &'static [Filter] = &[
        Filter::Active,
        Filter::Ready,
        Filter::Removed,
        Filter::Queued,
    ];

    pub fn label(&self) -> &str {
        match self {
            Self::Active => "Active",
            Self::Ready => "Ready",
            Self::Removed => "Removed",
            Self::Queued => "Queued",
        }
    }

    pub fn matches(&self, pr: &PullRequest) -> bool {
        match self {
            Self::Active => true,
            Self::Ready => pr.status == PrStatus::ReadyToMerge && !pr.is_draft,
            Self::Removed => pr.last_queue_removal.is_some() && pr.merge_queue.is_none(),
            Self::Queued => pr.merge_queue.is_some(),
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Active => Self::Ready,
            Self::Ready => Self::Removed,
            Self::Removed => Self::Queued,
            Self::Queued => Self::Active,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Active => Self::Queued,
            Self::Ready => Self::Active,
            Self::Removed => Self::Ready,
            Self::Queued => Self::Removed,
        }
    }
}

// ---------------------------------------------------------------------------
// LoadState / DiffState / Action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LoadState {
    Idle,
    Loading,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum DiffState {
    Loaded(Vec<FileDiff>),
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
    EnqueueSelected,
    ToggleSelectPr,
    SelectAllVisible,
    DeselectAll,
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
    NavigateTo(usize),
    ToggleDiff,
    FocusDiff,
    UnfocusDiff,
    DiffLoaded(u64, Vec<FileDiff>),
    DiffError(u64, String),
    DiffScrollUp(usize),
    DiffScrollDown(usize),
    PrEnqueued(u64, MergeQueueEntry),
    EnqueueFailed,
    BulkEnqueued(Vec<(u64, Result<MergeQueueEntry, String>)>),
    SetFilter(Filter),
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

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
    /// Whether the diff panel is visible.
    pub show_diff: bool,
    /// Whether j/k/arrows scroll the diff instead of navigating the PR list.
    pub diff_focused: bool,
    /// Cached diff per PR number.
    pub diff_cache: std::collections::HashMap<u64, DiffState>,
    /// Scroll offset within the diff panel.
    pub diff_scroll: usize,
    /// Height of the visible diff area in rows (updated each render frame).
    pub diff_height: usize,
    /// Clickable rects for each filter tab (parallel to Filter::ALL). Updated each render frame.
    pub filter_tab_rects: Vec<Rect>,
    /// Area of the diff panel. Updated each render frame; used for click-to-focus.
    pub diff_panel_rect: Rect,
    /// Area of the PR list items (below filter tabs). Updated each render frame; used for click-to-select.
    pub pr_list_rect: Rect,
    /// PR numbers toggled into the multi-select set.
    pub selected_prs: HashSet<u64>,
    /// True while a batch enqueue request is in flight.
    pub enqueue_in_flight: bool,
    /// Number of PRs dispatched in the current enqueue run (0 = idle).
    pub enqueue_total: usize,
    /// Incremented on every Tick — used to drive spinner frames.
    pub tick_count: usize,
}

impl App {
    pub fn new(config: Config, github: Arc<GitHubClient>, open_diff: bool) -> Self {
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
            show_diff: open_diff,
            diff_focused: false,
            diff_cache: std::collections::HashMap::new(),
            diff_scroll: 0,
            diff_height: 10,
            filter_tab_rects: Vec::new(),
            diff_panel_rect: Rect::default(),
            pr_list_rect: Rect::default(),
            selected_prs: HashSet::new(),
            enqueue_in_flight: false,
            enqueue_total: 0,
            tick_count: 0,
        }
    }

    /// PRs visible under the current filter.
    /// For the Queued filter, results are sorted by merge queue position ascending
    /// (longest in queue first). All other filters keep the global sort order.
    pub fn visible_prs(&self) -> Vec<&PullRequest> {
        let mut prs: Vec<&PullRequest> = self
            .prs
            .iter()
            .filter(|pr| self.active_filter.matches(pr))
            .collect();
        if self.active_filter == Filter::Queued {
            prs.sort_by_key(|pr| {
                pr.merge_queue
                    .as_ref()
                    .map(|e| e.position)
                    .unwrap_or(u32::MAX)
            });
        }
        prs
    }

    /// Count of PRs matching a given filter (for tab labels).
    pub fn count_for(&self, filter: &Filter) -> usize {
        self.prs.iter().filter(|pr| filter.matches(pr)).count()
    }

    /// Currently selected PR in the visible list.
    pub fn selected_pr(&self) -> Option<&PullRequest> {
        self.visible_prs().into_iter().nth(self.selected)
    }

    fn fetch_diff_if_needed(&self, pr_number: u64, tx: &UnboundedSender<Action>) {
        if self.diff_cache.contains_key(&pr_number) {
            return;
        }
        let github = Arc::clone(&self.github);
        let tx = tx.clone();
        tokio::spawn(async move {
            match github.fetch_diff(pr_number).await {
                Ok(files) => {
                    let _ = tx.send(Action::DiffLoaded(pr_number, files));
                }
                Err(e) => {
                    let _ = tx.send(Action::DiffError(pr_number, e.to_string()));
                }
            }
        });
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
            Event::Mouse(col, row) => {
                // Click on the diff panel focuses it (or unfocuses if already focused).
                if self.show_diff {
                    let r = &self.diff_panel_rect;
                    if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                        return Some(if self.diff_focused {
                            Action::UnfocusDiff
                        } else {
                            Action::FocusDiff
                        });
                    }
                }
                for (i, rect) in self.filter_tab_rects.iter().enumerate() {
                    if col >= rect.x
                        && col < rect.x + rect.width
                        && row >= rect.y
                        && row < rect.y + rect.height
                    {
                        if let Some(filter) = Filter::ALL.get(i) {
                            return Some(Action::SetFilter(filter.clone()));
                        }
                    }
                }
                // Click on a PR list item selects it.
                let r = &self.pr_list_rect;
                if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                    let offset = self.list_state.offset();
                    let idx = (row - r.y) as usize + offset;
                    return Some(Action::NavigateTo(idx));
                }
                None
            }

            Event::ScrollUp(col, row) => {
                if self.show_diff {
                    let r = &self.diff_panel_rect;
                    if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                        return Some(Action::DiffScrollUp(3));
                    }
                }
                Some(Action::NavigateUp)
            }

            Event::ScrollDown(col, row) => {
                if self.show_diff {
                    let r = &self.diff_panel_rect;
                    if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                        return Some(Action::DiffScrollDown(3));
                    }
                }
                Some(Action::NavigateDown)
            }
            Event::Key(code, modifiers) => {
                if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                    return Some(Action::Quit);
                }
                // When the diff panel has focus, navigation keys scroll the diff.
                if self.diff_focused {
                    return match code {
                        KeyCode::Up | KeyCode::Char('k') => Some(Action::DiffScrollUp(1)),
                        KeyCode::Down | KeyCode::Char('j') => Some(Action::DiffScrollDown(1)),
                        KeyCode::PageUp => Some(Action::DiffScrollUp(self.diff_height.max(1))),
                        KeyCode::PageDown => Some(Action::DiffScrollDown(self.diff_height.max(1))),
                        KeyCode::Home => Some(Action::DiffScrollUp(usize::MAX)),
                        KeyCode::End => Some(Action::DiffScrollDown(usize::MAX)),
                        KeyCode::Char('d') => Some(Action::UnfocusDiff),
                        KeyCode::Enter => Some(Action::ToggleDiff),
                        KeyCode::Char('R') => Some(Action::Refresh),
                        KeyCode::Char('o') => Some(Action::OpenInBrowser),
                        _ => None,
                    };
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
                    KeyCode::Char(' ') => Some(Action::ToggleSelectPr),
                    KeyCode::Char('a') => Some(Action::SelectAllVisible),
                    KeyCode::Char('A') => Some(Action::DeselectAll),
                    KeyCode::Char('r') => Some(Action::EnqueueSelected),
                    KeyCode::Char('R') => Some(Action::Refresh),
                    KeyCode::Char('o') => Some(Action::OpenInBrowser),
                    KeyCode::Enter => Some(Action::ToggleDiff),
                    KeyCode::Char('d') => {
                        if self.show_diff {
                            Some(if self.diff_focused {
                                Action::UnfocusDiff
                            } else {
                                Action::FocusDiff
                            })
                        } else {
                            None
                        }
                    }
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
                self.tick_count = self.tick_count.wrapping_add(1);
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
                    self.diff_scroll = 0;
                }
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::NavigateDown => {
                let count = self.visible_prs().len();
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                    self.list_state.select(Some(self.selected));
                    self.diff_scroll = 0;
                }
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::NavigatePageUp => {
                let page = self.list_height.max(1);
                self.selected = self.selected.saturating_sub(page);
                self.list_state.select(Some(self.selected));
                self.diff_scroll = 0;
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::NavigatePageDown => {
                let count = self.visible_prs().len();
                if count > 0 {
                    let page = self.list_height.max(1);
                    self.selected = (self.selected + page).min(count - 1);
                    self.list_state.select(Some(self.selected));
                    self.diff_scroll = 0;
                }
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::NavigateTo(idx) => {
                let count = self.visible_prs().len();
                if count > 0 {
                    self.selected = idx.min(count - 1);
                    self.list_state.select(Some(self.selected));
                    self.diff_scroll = 0;
                    if self.show_diff {
                        if let Some(pr) = self.selected_pr() {
                            self.fetch_diff_if_needed(pr.number, action_tx);
                        }
                    }
                }
            }

            Action::NavigateHome => {
                self.selected = 0;
                self.list_state.select(Some(0));
                self.diff_scroll = 0;
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::NavigateEnd => {
                let count = self.visible_prs().len();
                if count > 0 {
                    self.selected = count - 1;
                    self.list_state.select(Some(self.selected));
                    self.diff_scroll = 0;
                }
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
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

            Action::SetFilter(filter) => {
                if self.active_filter != filter {
                    self.active_filter = filter;
                    self.selected = 0;
                    self.clamp_selection();
                }
            }

            Action::ToggleSelectPr => {
                if let Some(pr) = self.selected_pr() {
                    let n = pr.number;
                    if !self.selected_prs.remove(&n) {
                        self.selected_prs.insert(n);
                    }
                }
            }

            Action::SelectAllVisible => {
                let numbers: Vec<u64> = self.visible_prs().iter().map(|pr| pr.number).collect();
                for n in numbers {
                    self.selected_prs.insert(n);
                }
            }

            Action::DeselectAll => {
                self.selected_prs.clear();
            }

            Action::EnqueueSelected => {
                if self.enqueue_in_flight {
                    return Ok(());
                }
                if !self.selected_prs.is_empty() {
                    // Batch path: enqueue all selected PRs not already in the queue.
                    let targets: Vec<(u64, String)> = self
                        .prs
                        .iter()
                        .filter(|pr| {
                            self.selected_prs.contains(&pr.number) && pr.merge_queue.is_none()
                        })
                        .map(|pr| (pr.number, pr.node_id.clone()))
                        .collect();
                    self.selected_prs.clear();
                    if !targets.is_empty() {
                        self.enqueue_in_flight = true;
                        self.enqueue_total = targets.len();
                        let github = Arc::clone(&self.github);
                        let tx = action_tx.clone();
                        tokio::spawn(async move {
                            let results = github.enqueue_prs(&targets).await;
                            let converted = results
                                .into_iter()
                                .map(|(n, r)| (n, r.map_err(|e| e.to_string())))
                                .collect();
                            let _ = tx.send(Action::BulkEnqueued(converted));
                        });
                    }
                } else if let Some(pr) = self.selected_pr() {
                    // Single-PR fallback path.
                    if pr.merge_queue.is_none() {
                        let github = Arc::clone(&self.github);
                        let node_id = pr.node_id.clone();
                        let pr_number = pr.number;
                        let tx = action_tx.clone();
                        self.enqueue_in_flight = true;
                        self.enqueue_total = 1;
                        tokio::spawn(async move {
                            match github.enqueue_pr(&node_id).await {
                                Ok(entry) => {
                                    let _ = tx.send(Action::PrEnqueued(pr_number, entry));
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::StatusMessage(format!(
                                        "Error queuing PR #{pr_number}: {e}"
                                    )));
                                    let _ = tx.send(Action::EnqueueFailed);
                                }
                            }
                        });
                    }
                }
            }

            Action::BulkEnqueued(results) => {
                self.enqueue_in_flight = false;
                let mut ok_count = 0usize;
                let mut errors: Vec<String> = Vec::new();
                for (pr_number, result) in results {
                    match result {
                        Ok(entry) => {
                            ok_count += 1;
                            if let Some(pr) = self.prs.iter_mut().find(|p| p.number == pr_number) {
                                pr.merge_queue = Some(entry);
                                pr.status = PullRequest::compute_status(
                                    &pr.mergeable_state,
                                    &pr.merge_queue,
                                    pr.is_draft,
                                    &pr.last_queue_removal,
                                );
                            }
                        }
                        Err(msg) => {
                            errors.push(format!("#{pr_number}: {msg}"));
                        }
                    }
                }
                self.clamp_selection();
                let status = if errors.is_empty() {
                    format!(
                        "{ok_count} PR{} added to merge queue",
                        if ok_count == 1 { "" } else { "s" }
                    )
                } else {
                    format!(
                        "{ok_count} PR{} added; {} failed",
                        if ok_count == 1 { "" } else { "s" },
                        errors.len()
                    )
                };
                self.status_msg = Some((status, Instant::now()));
            }

            Action::EnqueueFailed => {
                self.enqueue_in_flight = false;
            }

            Action::PrEnqueued(pr_number, entry) => {
                self.enqueue_in_flight = false;
                // Update the PR in-place — no full reload needed.
                if let Some(pr) = self.prs.iter_mut().find(|p| p.number == pr_number) {
                    pr.merge_queue = Some(entry);
                    pr.status = PullRequest::compute_status(
                        &pr.mergeable_state,
                        &pr.merge_queue,
                        pr.is_draft,
                        &pr.last_queue_removal,
                    );
                }
                self.clamp_selection();
                self.status_msg = Some((
                    format!("PR #{pr_number} added to merge queue"),
                    Instant::now(),
                ));
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

            Action::ToggleDiff => {
                if self.show_diff {
                    // Close the panel entirely (regardless of focus state).
                    self.show_diff = false;
                    self.diff_focused = false;
                } else {
                    // Open the diff panel without stealing focus.
                    self.show_diff = true;
                    self.diff_focused = false;
                    self.diff_scroll = 0;
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
            }

            Action::FocusDiff => {
                self.diff_focused = true;
            }

            Action::UnfocusDiff => {
                self.diff_focused = false;
            }

            Action::DiffLoaded(pr_number, files) => {
                self.diff_cache.insert(pr_number, DiffState::Loaded(files));
            }

            Action::DiffError(pr_number, msg) => {
                self.diff_cache.insert(pr_number, DiffState::Error(msg));
            }

            Action::DiffScrollUp(amount) => {
                self.diff_scroll = self.diff_scroll.saturating_sub(amount);
            }

            Action::DiffScrollDown(amount) => {
                self.diff_scroll = self.diff_scroll.saturating_add(amount);
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
                if self.show_diff {
                    if let Some(pr) = self.selected_pr() {
                        self.fetch_diff_if_needed(pr.number, action_tx);
                    }
                }
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
