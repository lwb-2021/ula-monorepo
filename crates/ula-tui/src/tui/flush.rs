use ratatui::{
    Terminal,
    backend::Backend,
    buffer::{Buffer, CellWidth},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget, Wrap},
};

use crate::app::Turn;
use crate::tui::draw::push_styled;

/// Pushes a finished turn into the scrollback above the inline viewport.
pub fn turn<B>(terminal: &mut Terminal<B>, turn: &Turn) -> anyhow::Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let width = terminal.get_frame().area().width;
    if width == 0 {
        return Ok(());
    }

    let mut lines = Vec::new();
    match turn {
        Turn::User(text) => push_user(&mut lines, text),
        Turn::Assistant { reasoning, content } => {
            if reasoning
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
            {
                lines.push(Line::from(Span::styled(
                    "✻ 思考",
                    Style::default().fg(Color::Magenta),
                )));
            }
            push_styled(&mut lines, content, Style::default());
        }
    }
    lines.push(Line::default());

    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    let height = paragraph.line_count(width) as u16;
    if height == 0 {
        return Ok(());
    }

    terminal.insert_before(height, |buffer| {
        paragraph.render(buffer.area, buffer);
        hide_wide_trailing(buffer);
    })?;
    Ok(())
}

/// `Terminal::insert_before` sends every cell to the backend verbatim, unlike the
/// normal frame diff which skips the columns a wide grapheme already covers. Those
/// trailing cells are `Cell::EMPTY`, whose `symbol()` is a space, so they would land
/// in the scrollback as a gap after every CJK character / emoji.
fn hide_wide_trailing(buffer: &mut Buffer) {
    let area = buffer.area;
    for y in area.top()..area.bottom() {
        let mut x = area.left();
        while x < area.right() {
            let width = buffer[(x, y)].symbol().cell_width().max(1);
            for dx in 1..width {
                if let Some(cell) = buffer.cell_mut((x + dx, y)) {
                    cell.set_symbol("");
                }
            }
            x += width;
        }
    }
}

fn push_user(lines: &mut Vec<Line<'static>>, text: &str) {
    let marker = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    for (index, line) in text.split('\n').enumerate() {
        let prefix = if index == 0 { "❯ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, marker),
            Span::raw(line.to_string()),
        ]));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Paragraph};

    use super::*;

    #[test]
    fn hide_wide_trailing_blanks_only_continuation_cells() {
        let area = Rect::new(0, 0, 8, 1);
        let mut buffer = Buffer::empty(area);
        Paragraph::new("中 a").render(area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "中");
        assert_eq!(buffer[(1, 0)].symbol(), " ");

        hide_wide_trailing(&mut buffer);

        assert!(buffer[(1, 0)].symbol().is_empty());
        assert_eq!(buffer[(2, 0)].symbol(), " ", "real spaces must survive");
        assert_eq!(buffer[(3, 0)].symbol(), "a");
    }
}
