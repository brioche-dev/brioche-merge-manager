use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph},
    Frame,
};

use crate::{
    app::{App, Filter, LoadState},
    github::models::PrStatus,
};

pub fn render_pr_list(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            "Pull Requests".cyan().bold(),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Color::DarkGray);

    match &app.load_state.clone() {
        LoadState::Loading => {
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let inner = Rect {
                x: area.x + 1,
                y: area.y + area.height / 2,
                width: area.width.saturating_sub(2),
                height: 1,
            };
            let (loaded, total) = app.load_progress.unwrap_or((0, 0));
            let ratio = if total > 0 {
                (loaded as f64 / total as f64).min(1.0)
            } else {
                0.0
            };
            let label = if total > 0 {
                format!("  Loading… {loaded} / {total} PRs")
            } else {
                "  Loading…".to_string()
            };
            let gauge = Gauge::default()
                .gauge_style(Style::new().cyan().on_dark_gray())
                .ratio(ratio)
                .label(label);
            f.render_widget(gauge, inner);
            return;
        }

        LoadState::Error(msg) => {
            let msg = msg.clone();
            f.render_widget(Clear, area);
            let error_block = Block::default()
                .title(Line::from(vec![
                    Span::raw(" "),
                    "Error".red().bold(),
                    Span::raw(" "),
                ]))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Color::Red);
            let error_text = Paragraph::new(format!("{msg}\n\nPress [R] to retry"))
                .block(error_block)
                .style(Style::new().red());
            f.render_widget(error_text, area);
            return;
        }

        LoadState::Idle => {}
    }

    // Render the outer block, then split inner area:
    //   [1 line]  filter tab bar
    //   [rest]    scrollable PR list
    f.render_widget(block, area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        });

    render_filter_tabs(f, app, inner[0]);

    // Keep app in sync so PageUp/Down know how many rows are visible
    app.list_height = inner[1].height as usize;

    let visible = app.visible_prs();

    if visible.is_empty() {
        let label = app.active_filter.label().to_lowercase();
        let msg = Paragraph::new(Line::from(vec![
            Span::raw("  "),
            format!("No {label} PRs").dim(),
        ]));
        f.render_widget(msg, inner[1]);
        return;
    }

    let items: Vec<ListItem> = visible
        .iter()
        .map(|pr| {
            let (dot_color, status_hint) = match pr.status {
                PrStatus::ReadyToMerge => (Color::Green, "ready "),
                PrStatus::FailedMerge => (Color::Red, "failed"),
                PrStatus::InQueue => (Color::Yellow, "queue "),
            };

            let (rollup_sym, rollup_color, rollup_dim) = match &pr.check_rollup {
                Some(r) => (r.symbol().to_owned(), r.color(), false),
                None => ("—".to_owned(), Color::Reset, true),
            };

            let draft_marker = if pr.is_draft {
                " draft".dim().italic()
            } else {
                Span::raw("")
            };

            let rollup_span: Span = if rollup_dim {
                rollup_sym.dim()
            } else {
                rollup_sym.fg(rollup_color)
            };

            let line = Line::from(vec![
                Span::raw(" "),
                "●".fg(dot_color),
                format!(" #{:<5}", pr.number).dim(),
                format!(" {status_hint} ").fg(dot_color),
                rollup_span,
                Span::raw("  "),
                draft_marker,
                Span::raw(pr.title.clone()),
                format!("  @{}", pr.author).dim(),
            ]);

            ListItem::new(line)
        })
        .collect();

    drop(visible); // release the immutable borrow on app before the mutable borrow below

    let list = List::new(items)
        .highlight_style(Style::new().black().on_cyan().bold())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, inner[1], &mut app.list_state);
}

fn render_filter_tabs(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];

    for filter in Filter::ALL {
        let count = app.count_for(filter);
        let is_active = *filter == app.active_filter;
        let label = format!(" {} ({}) ", filter.label(), count);

        if is_active {
            spans.push("▶".cyan().on_cyan());
            spans.push(label.black().on_cyan().bold());
        } else {
            spans.push(label.dim());
        }
        spans.push(Span::raw(" "));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
