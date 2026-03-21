mod pr_detail;
mod pr_diff;
mod pr_list;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Stylize,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App) {
    // Header and legend always span full width.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header + breathing room
            Constraint::Fill(1),   // main content
            Constraint::Length(3), // legend
        ])
        .split(f.area());

    render_header(f, app, rows[0]);
    render_legend(f, app, rows[2]);

    // Filter tabs always occupy a 1-line row above the panels so their
    // position never shifts when the diff panel is toggled.
    let main_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Fill(1)])
        .split(rows[1]);

    pr_list::render_filter_tabs(f, app, main_rows[0]);

    if app.show_diff {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(2), Constraint::Fill(3)])
            .split(main_rows[1]);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(2), Constraint::Fill(3)])
            .split(cols[0]);

        app.diff_height = cols[1].height.saturating_sub(2) as usize;
        app.diff_panel_rect = cols[1];

        pr_list::render_pr_list(f, app, left[0]);
        pr_detail::render_pr_detail(f, app, left[1]);
        pr_diff::render_pr_diff(f, app, cols[1]);
    } else {
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(2), Constraint::Fill(3)])
            .split(main_rows[1]);

        pr_list::render_pr_list(f, app, left[0]);
        pr_detail::render_pr_detail(f, app, left[1]);
    }
}

fn render_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let debug_badge = if tracing::enabled!(tracing::Level::DEBUG) {
        " [DBG]"
    } else {
        ""
    };
    let pr_count = if app.prs.is_empty() {
        String::new()
    } else {
        format!("  ·  {} PRs", app.prs.len())
    };

    let line = Line::from(vec![
        " 🍞 Brioche Merge Manager ".black().on_cyan().bold(),
        format!("  {}/{}{}", app.config.owner, app.config.repo, pr_count).dim(),
        debug_badge.red().bold(),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

fn render_legend(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let status_line = if app.enqueue_in_flight {
        let frame = SPINNER[app.tick_count % SPINNER.len()];
        let msg = if app.enqueue_total == 1 {
            "  Adding PR to merge queue…".to_string()
        } else {
            format!("  Adding {} PRs to merge queue…", app.enqueue_total)
        };
        Line::from(vec![
            Span::raw(" "),
            frame.yellow().bold(),
            msg.yellow().bold(),
        ])
    } else if let Some((msg, _)) = &app.status_msg {
        Line::from(vec![
            Span::raw(" "),
            "▶ ".yellow(),
            msg.as_str().yellow().bold(),
        ])
    } else {
        Line::raw("")
    };

    let (nav_line, action_line) = if app.diff_focused {
        (
            Line::from(vec![
                Span::raw(" "),
                key("↑↓"),
                sep(" / "),
                key("jk"),
                desc(" Scroll diff  "),
                key("PgUp"),
                sep("/"),
                key("PgDn"),
                desc(" Page  "),
                key("Home"),
                sep("/"),
                key("End"),
                desc(" Top/Bottom"),
            ]),
            Line::from(vec![
                Span::raw(" "),
                key("d"),
                desc(" Unfocus diff  "),
                key("Enter"),
                desc(" Close diff  "),
                key("o"),
                desc(" Open in browser  "),
                key("R"),
                desc(" Refresh  "),
                key("Ctrl+C"),
                desc(" Quit"),
            ]),
        )
    } else {
        (
            Line::from(vec![
                Span::raw(" "),
                key("↑↓"),
                sep(" / "),
                key("jk"),
                desc(" Navigate  "),
                key("Tab"),
                sep(" / "),
                key("⇧Tab"),
                desc(" Cycle filter  "),
                key("R"),
                desc(" Refresh"),
            ]),
            Line::from(vec![
                Span::raw(" "),
                key("Space"),
                desc(" Select  "),
                key("a"),
                desc(" Select all  "),
                key("A"),
                desc(" Deselect all  "),
                key("r"),
                desc(" Add to queue  "),
                key("o"),
                desc(" Open  "),
                key("Enter"),
                desc(" Toggle diff  "),
                key("Ctrl+C"),
                desc(" Quit"),
            ]),
        )
    };

    f.render_widget(
        Paragraph::new(vec![status_line, nav_line, action_line]),
        area,
    );
}

fn key(k: &str) -> Span<'_> {
    k.cyan().bold()
}

fn sep(s: &str) -> Span<'_> {
    s.dim()
}

fn desc(d: &str) -> Span<'_> {
    d.dim()
}
