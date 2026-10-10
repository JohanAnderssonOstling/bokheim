//! A shared launch owns one availability prompt, including across windows.
#[derive(Default)]
pub(super) struct UpdatePromptState {
    shown: bool,
}

impl UpdatePromptState {
    pub(super) fn take(&mut self, available: bool) -> bool {
        if !available || self.shown {
            return false;
        }
        self.shown = true;
        true
    }
}
