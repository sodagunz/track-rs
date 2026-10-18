use super::{track_number, App, Block, Borders, Frame, Line, Paragraph, Rect};
use crate::synth_controls::CONTROLS;

pub(super) fn draw(f: &mut Frame, area: Rect, app: &App) {
    let selected = app.cursor_row.min(CONTROLS.len() - 1);
    let control = CONTROLS[selected];
    let block = Block::default().borders(Borders::ALL).title(format!(
        "> Synth (all patterns) | T{} | {} | {}",
        track_number(app.cursor_track),
        control.group(),
        app.mode.label()
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = usize::from(inner.height);
    let first = selected
        .saturating_sub(height / 2)
        .min(CONTROLS.len().saturating_sub(height));
    let patch = app.project.synth_patch(app.cursor_track);
    let lines: Vec<_> = CONTROLS
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(row, control)| {
            Line::styled(
                format!(
                    "{} {} {} {} | {}",
                    if row == selected { ">" } else { " " },
                    control.group(),
                    control.label(),
                    control.value(patch),
                    control.policy()
                ),
                if row == selected {
                    app.theme.cursor()
                } else {
                    app.theme.base()
                },
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}
