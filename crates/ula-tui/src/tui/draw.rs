use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

use crate::app::{App, Scroll};
use crate::tui::{INPUT_H, overlay};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [live_area, input_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(INPUT_H)]).areas(area);

    render_live(frame, app, live_area);
    render_input(frame, app, input_area);

    if let Some(modal) = &app.modal {
        overlay::render(frame, modal);
    }
}

fn render_live(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.busy {
        Line::from(Span::styled(
            " 生成中…",
            Style::default().fg(Color::Yellow),
        ))
    } else {
        Line::from(" ula")
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // One column is reserved for the scrollbar.
    let text_width = inner.width.saturating_sub(1);
    let text_area = Rect::new(inner.x, inner.y, text_width, inner.height);

    let paragraph = Paragraph::new(Text::from(live_lines(app))).wrap(Wrap { trim: false });
    let total = paragraph.line_count(text_width);
    sync_scroll(&mut app.scroll, total, inner.height);
    frame.render_widget(paragraph.scroll((app.scroll.offset, 0)), text_area);

    if total > inner.height as usize {
        let mut state = ScrollbarState::new(total)
            .position(app.scroll.offset as usize)
            .viewport_content_length(inner.height as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut state,
        );
    }
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" 输入 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 3 {
        return;
    }

    let available = (inner.width - 3) as usize;
    let scroll = app.input.visual_scroll(available.max(1));
    let text = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(app.input.value().to_string()),
    ]);
    frame.render_widget(Paragraph::new(text).scroll((0, scroll as u16)), inner);

    if app.modal.is_none() {
        let cursor = (inner.x + 2 + (app.input.visual_cursor() - scroll) as u16)
            .min(inner.x + inner.width - 1);
        frame.set_cursor_position((cursor, inner.y));
    }
}

fn sync_scroll(scroll: &mut Scroll, total: usize, viewport: u16) {
    let max = total.saturating_sub(viewport as usize) as u16;
    if scroll.follow || scroll.offset >= max {
        scroll.follow = true;
        scroll.offset = max;
    } else {
        scroll.offset = scroll.offset.min(max);
    }
}

fn live_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(error) = &app.error {
        push_styled(&mut lines, error, Style::default().fg(Color::Red));
        lines.push(Line::default());
    }
    if !app.live.reasoning.is_empty() {
        push_styled(
            &mut lines,
            &app.live.reasoning,
            Style::default().fg(Color::DarkGray),
        );
        lines.push(Line::default());
    }
    push_styled(&mut lines, &app.live.content, Style::default());
    lines
}

/// Splits `text` into physical lines, all carrying `style`.
/// Leading and trailing line breaks are dropped so model output that starts
/// or ends with blank lines does not inflate the line spacing.
pub(super) fn push_styled(lines: &mut Vec<Line<'static>>, text: &str, style: Style) {
    lines.extend(
        text.trim_matches(&['\n', '\r'][..])
            .split('\n')
            .map(|line| Line::from(Span::styled(line.to_string(), style))),
    );
}
