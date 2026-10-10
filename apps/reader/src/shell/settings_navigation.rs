//! Row selection and immediate arrow-key editing in reader settings.

use std::{cell::Cell, rc::Rc};

use gpui::{App, Div, canvas, prelude::*};
use ui_components as components;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsRow {
    #[default]
    Theme,
    Typeface,
    Alignment,
    FontSize,
    LineHeight,
    SideMargins,
    VerticalMargins,
    ColumnGap,
    MinLineWidth,
    MaxLineWidth,
    PdfMargins,
    PdfFit,
    PdfPages,
    PdfZoom,
}

#[derive(Default)]
pub(crate) struct SettingsNavigation {
    selected: SettingsRow,
    scroll_pending: Rc<Cell<bool>>,
}

impl SettingsNavigation {
    pub(crate) fn selected(&self, rows: &[SettingsRow]) -> SettingsRow {
        if rows.contains(&self.selected) { self.selected } else { rows[0] }
    }

    pub(crate) fn move_row(&mut self, rows: &[SettingsRow], direction: i32) {
        let index = rows.iter().position(|row| *row == self.selected(rows)).unwrap_or(0);
        let next = (index as i32 + direction).clamp(0, rows.len() as i32 - 1) as usize;
        self.selected = rows[next];
        self.scroll_pending.set(true);
    }

    /// Marks the selected row and scrolls it into view after keyboard movement.
    pub(crate) fn row(&self, row: SettingsRow, rows: &[SettingsRow], content: Div, theme: components::BrowserTheme) -> Div {
        let selected = self.selected(rows) == row;
        let pending = self.scroll_pending.clone();
        content.relative().border_l_2().border_color(if selected { theme.accent } else { gpui::transparent_black() }).child(
            canvas(
                move |bounds, window, _| {
                    if selected && pending.replace(false) {
                        window.request_autoscroll(bounds);
                    }
                },
                |_, (), _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }
}

/// Choice rows stop at their ends, like numeric settings stop at their bounds.
pub(crate) fn adjacent_value<T: Copy + PartialEq>(values: &[T], current: T, direction: i32) -> T {
    let index = values.iter().position(|value| *value == current).unwrap_or(0) as i32;
    values[(index + direction).clamp(0, values.len() as i32 - 1) as usize]
}

pub(crate) fn adjust_theme(direction: i32, cx: &mut App) {
    let choices = components::THEME_CHOICES;
    let current = cx.try_global::<components::theme::SelectedPalette>().map(|selection| (selection.0, selection.1)).unwrap_or(choices[0]);
    let (tint, appearance) = adjacent_value(&choices, current, direction);
    components::theme::select_palette(tint, appearance, cx);
}

#[cfg(test)]
mod tests {
    use super::{SettingsNavigation, SettingsRow, adjacent_value};

    #[test]
    fn row_selection_clamps_and_handles_conditional_rows() {
        let mut navigation = SettingsNavigation::default();
        let rows = [SettingsRow::PdfMargins, SettingsRow::PdfFit, SettingsRow::PdfPages];
        assert_eq!(navigation.selected(&rows), SettingsRow::PdfMargins);
        navigation.move_row(&rows, -1);
        assert_eq!(navigation.selected(&rows), SettingsRow::PdfMargins);
        navigation.move_row(&rows, 1);
        navigation.move_row(&rows, 1);
        navigation.move_row(&rows, 1);
        assert_eq!(navigation.selected(&rows), SettingsRow::PdfPages);
        let changed = [SettingsRow::PdfMargins, SettingsRow::PdfFit, SettingsRow::PdfZoom];
        assert_eq!(navigation.selected(&changed), SettingsRow::PdfMargins);
    }

    #[test]
    fn arrow_choices_apply_the_adjacent_value_and_stop_at_limits() {
        let values = [10, 20, 30];
        assert_eq!(adjacent_value(&values, 20, -1), 10);
        assert_eq!(adjacent_value(&values, 20, 1), 30);
        assert_eq!(adjacent_value(&values, 10, -1), 10);
        assert_eq!(adjacent_value(&values, 30, 1), 30);
    }
}
