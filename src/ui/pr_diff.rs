use ratatui::{
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::{
    app::{App, DiffState},
    github::models::FileDiff,
};

pub fn render_pr_diff(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.diff_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = if app.diff_focused {
        Line::from(vec![
            Span::raw(" "),
            "Diff".black().on_cyan().bold(),
            "  ↑↓/jk scroll  d unfocus  Enter close".dim(),
            Span::raw(" "),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            "Diff".cyan().bold(),
            "  d to focus".dim(),
            Span::raw(" "),
        ])
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_color);

    let Some(pr) = app.selected_pr() else {
        f.render_widget(block, area);
        return;
    };

    match app.diff_cache.get(&pr.number) {
        None => {
            let lines = vec![Line::raw(""), Line::from("  Loading…".dim())];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }

        Some(DiffState::Error(msg)) => {
            let lines = vec![Line::raw(""), Line::from(format!("  Error: {msg}").red())];
            f.render_widget(Paragraph::new(lines).block(block), area);
        }

        Some(DiffState::Loaded(files)) => {
            let inner_width = area.width.saturating_sub(2);
            let total_add: u32 = files.iter().map(|f| f.additions).sum();
            let total_del: u32 = files.iter().map(|f| f.deletions).sum();

            // project.bri files sort to the top; everything else keeps its original order.
            let mut sorted_files: Vec<&FileDiff> = files.iter().collect();
            sorted_files.sort_by_key(|f| {
                let name = f.filename.as_str();
                u8::from(name != "project.bri" && !name.ends_with("/project.bri"))
            });

            let summary = Line::from(vec![
                Span::raw("  "),
                format!("{} files", files.len()).bold(),
                Span::raw("  "),
                format!("+{total_add}").green().bold(),
                Span::raw("  "),
                format!("-{total_del}").red().bold(),
            ]);

            let mut lines: Vec<Line> = vec![
                Line::raw(""),
                summary,
                Line::from("  ──────────────────────────────────────".dark_gray()),
            ];

            for file in sorted_files {
                lines.extend(file_lines(file, inner_width));
            }

            let visible_height = area.height.saturating_sub(2) as usize;
            let max_scroll = lines.len().saturating_sub(visible_height);
            let scroll = app.diff_scroll.min(max_scroll) as u16;

            f.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);
        }
    }
}

fn file_lines<'a>(file: &'a FileDiff, width: u16) -> Vec<Line<'a>> {
    let name_max = width.saturating_sub(16) as usize;
    let name = truncate_left(&file.filename, name_max);

    let header = Line::from(vec![
        Span::raw(" "),
        file.status.symbol().fg(file.status.color()).bold(),
        Span::raw(" "),
        name.bold(),
        Span::raw("  "),
        format!("+{}", file.additions).green(),
        Span::raw(" "),
        format!("-{}", file.deletions).red(),
    ]);

    let mut out = vec![Line::raw(""), header];

    if let Some(patch) = &file.patch {
        for raw in patch.lines() {
            out.push(patch_line(raw, width));
        }
    } else {
        out.push(Line::from("  binary".dim()));
    }

    out
}

fn patch_line(raw: &str, width: u16) -> Line<'_> {
    let max = width.saturating_sub(1) as usize;
    let display = truncate_str(raw, max);

    if display.starts_with("@@") {
        Line::from(display.to_string().cyan().dim())
    } else if display.starts_with('+') {
        Line::from(display.to_string().green())
    } else if display.starts_with('-') {
        Line::from(display.to_string().red())
    } else {
        Line::from(display.to_string().dim())
    }
}

/// Truncate a string to at most `max` chars, preserving UTF-8 boundaries.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Walk char boundaries to find the largest safe byte index ≤ max.
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Truncate from the left, prepending "…" if the name is too long.
fn truncate_left(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max || max == 0 {
        return s.to_string();
    }
    let skip = char_count - max + 1; // +1 for the "…"
    format!("…{}", s.chars().skip(skip).collect::<String>())
}
