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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Fill(2),   // pr list
            Constraint::Fill(3),   // pr detail / diff
            Constraint::Length(3), // legend
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);
    pr_list::render_pr_list(f, app, chunks[1]);

    if app.show_diff {
        let detail_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(3), Constraint::Fill(2)])
            .split(chunks[2]);
        // Keep diff_height in sync so PageUp/Down scroll the right amount.
        app.diff_height = detail_chunks[1].height.saturating_sub(2) as usize;
        pr_detail::render_pr_detail(f, app, detail_chunks[0]);
        pr_diff::render_pr_diff(f, app, detail_chunks[1]);
    } else {
        pr_detail::render_pr_detail(f, app, chunks[2]);
    }

    render_legend(f, app, chunks[3]);
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
    let status_line = if let Some((msg, _)) = &app.status_msg {
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
                key("Esc"),
                desc(" Unfocus diff  "),
                key("d"),
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
                key("q"),
                desc(" Queue PR  "),
                key("r"),
                desc(" Retry PR  "),
                key("o"),
                desc(" Open in browser  "),
                key("d"),
                desc(" Diff  "),
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
