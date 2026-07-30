use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::modal::Modal;

pub fn render(frame: &mut Frame, modal: &Modal) {
    let area = frame.area();
    if area.width < 16 || area.height < 5 {
        return;
    }

    let (title, body) = content(modal);
    let width = area.width.saturating_sub(4).min(64);
    let probe = Paragraph::new(Text::from(body.clone())).wrap(Wrap { trim: false });
    let height = (probe.line_count(width - 2) as u16 + 2).min(area.height - 2);

    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(Text::from(body))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        rect,
    );
}

fn content(modal: &Modal) -> (&'static str, Vec<Line<'static>>) {
    let title = modal.title();
    let body = match modal {
        Modal::Confirm {
            request_type,
            message,
            ..
        } => {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("类型 ", Style::default().fg(Color::DarkGray)),
                    Span::raw(request_type.clone()),
                ]),
                Line::default(),
            ];
            lines.extend(
                message
                    .split('\n')
                    .map(|line| Line::from(line.to_string())),
            );
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(
                    "y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 允许    "),
                Span::styled(
                    "n",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 拒绝"),
            ]));
            lines
        }
    };
    (title, body)
}
