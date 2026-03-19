use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::{
    app::App,
    github::models::{MergeQueueState, PrStatus},
};

pub fn render_pr_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(pr) = app.selected_pr() else {
        let block = Block::default()
            .title(Line::from(vec![
                Span::raw(" "),
                "PR Detail".dim(),
                Span::raw(" "),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Color::DarkGray);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("  "),
                "No PR selected".dim(),
            ]))
            .block(block),
            area,
        );
        return;
    };

    let (status_text, status_color, status_dot) = match pr.status {
        PrStatus::ReadyToMerge => ("Ready to merge", Color::Green, "●"),
        PrStatus::FailedMerge  => ("Failed to merge", Color::Red, "●"),
        PrStatus::InQueue      => ("In merge queue", Color::Yellow, "●"),
    };

    let draft_badge = if pr.is_draft {
        "  draft".dim().italic()
    } else {
        Span::raw("")
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            status_dot.fg(status_color),
            format!(" PR #{}", pr.number).bold(),
            draft_badge,
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(status_color);

    // Labels use DIM so they recede without forcing a specific color
    let label_style = Style::new().dim();
    let sep_style   = Style::new().dark_gray();

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Title   ", label_style),
            Span::styled("│", sep_style),
            Span::raw("  "),
            pr.title.clone().bold(),
        ]),
        Line::from(vec![
            Span::styled("  Author  ", label_style),
            Span::styled("│", sep_style),
            Span::raw("  "),
            format!("@{}", pr.author).cyan(),
        ]),
        Line::from(vec![
            Span::styled("  Status  ", label_style),
            Span::styled("│", sep_style),
            Span::raw("  "),
            "●  ".fg(status_color),
            status_text.fg(status_color).bold(),
        ]),
    ];

    // Checks rollup
    let (rollup_symbol, rollup_label, rollup_color) = match &pr.check_rollup {
        Some(r) => (r.symbol(), r.label(), r.color()),
        None => ("—", "no checks", Color::Reset),
    };
    let rollup_span: Span = if rollup_color == Color::Reset {
        format!("{rollup_symbol}  {rollup_label}").dim()
    } else {
        format!("{rollup_symbol}  {rollup_label}").fg(rollup_color)
    };
    lines.push(Line::from(vec![
        Span::styled("  Checks  ", label_style),
        Span::styled("│", sep_style),
        Span::raw("  "),
        rollup_span,
    ]));

    // Review decision
    if let Some(review) = &pr.review_decision {
        lines.push(Line::from(vec![
            Span::styled("  Review  ", label_style),
            Span::styled("│", sep_style),
            Span::raw("  "),
            format!("{}  {}", review.symbol(), review.label()).fg(review.color()),
        ]));
    }

    // Merge queue info
    if let Some(entry) = &pr.merge_queue {
        let state_label = match entry.state {
            MergeQueueState::Queued      => "queued",
            MergeQueueState::Awaiting    => "awaiting checks",
            MergeQueueState::Mergeable   => "mergeable",
            MergeQueueState::Unmergeable => "unmergeable",
            MergeQueueState::Locked      => "locked",
        };
        let state_color = match entry.state {
            MergeQueueState::Mergeable   => Color::Green,
            MergeQueueState::Unmergeable => Color::Red,
            _                            => Color::Yellow,
        };
        lines.push(Line::from(vec![
            Span::styled("  Queue   ", label_style),
            Span::styled("│", sep_style),
            Span::raw("  "),
            Span::raw(format!("position {}  ", entry.position)),
            state_label.fg(state_color),
        ]));
    }

    // URL (truncated to fit)
    let url_max = area.width.saturating_sub(14) as usize;
    let url_display = if pr.html_url.len() > url_max {
        format!("{}…", &pr.html_url[..url_max.saturating_sub(1)])
    } else {
        pr.html_url.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("  URL     ", label_style),
        Span::styled("│", sep_style),
        Span::raw("  "),
        url_display.dim(),
    ]));

    // Separator before actions
    lines.push(Line::raw(""));
    lines.push(Line::from(
        "  ─────────────────────────────────────────────────────".dark_gray(),
    ));
    lines.push(Line::raw(""));

    // Action hints — dim inapplicable ones rather than forcing a dark color
    let queue_active = pr.status == PrStatus::ReadyToMerge;
    let retry_active = pr.status == PrStatus::FailedMerge;

    let key_q: Span = if queue_active { "q".cyan().bold() } else { "q".dim() };
    let desc_q: Span = if queue_active { Span::raw("  Queue PR") } else { "  Queue PR".dim() };
    let key_r: Span = if retry_active { "r".cyan().bold() } else { "r".dim() };
    let desc_r: Span = if retry_active { Span::raw("  Retry PR") } else { "  Retry PR".dim() };

    lines.push(Line::from(vec![
        Span::raw("  "),
        key_q,
        desc_q,
        Span::raw("     "),
        key_r,
        desc_r,
        Span::raw("     "),
        "o".cyan().bold(),
        Span::raw("  Open in browser"),
    ]));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
