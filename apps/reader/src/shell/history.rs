//! Browser-style history of deliberate jumps, independent of page turning.

#[derive(Clone)]
pub(crate) struct NavigationHistory<T> {
    back: Vec<T>,
    forward: Vec<T>,
    pending_origin: Option<T>,
}

impl<T> Default for NavigationHistory<T> {
    fn default() -> Self {
        Self { back: Vec::new(), forward: Vec::new(), pending_origin: None }
    }
}

impl<T: PartialEq> NavigationHistory<T> {
    pub(crate) fn availability(&self) -> (bool, bool) {
        (!self.back.is_empty(), !self.forward.is_empty())
    }

    /// A jump is committed only when a settled viewport actually moves. Keep
    /// the first origin if several requests arrive before the next frame.
    pub(crate) fn begin_jump(&mut self, origin: T) {
        self.pending_origin.get_or_insert(origin);
    }

    pub(crate) fn settle(&mut self, position: &T) -> bool {
        if let Some(origin) = self.pending_origin.take().filter(|origin| origin != position) {
            self.back.push(origin);
            self.forward.clear();
            true
        } else {
            false
        }
    }

    pub(crate) fn navigate(&mut self, current: T, forward: bool) -> Option<T> {
        self.settle(&current);
        let (source, destination) = if forward { (&mut self.forward, &mut self.back) } else { (&mut self.back, &mut self.forward) };
        while let Some(target) = source.pop() {
            if target != current {
                destination.push(current);
                return Some(target);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::NavigationHistory;

    #[test]
    fn back_and_forward_preserve_reading_done_after_each_jump() {
        let mut history = NavigationHistory::default();
        history.settle(&8);
        history.settle(&9); // Ordinary reading creates no history.
        assert_eq!(history.availability(), (false, false));
        history.begin_jump(9);
        history.settle(&2);
        history.settle(&3);
        assert_eq!(history.navigate(3, false), Some(9));
        history.settle(&10);
        assert_eq!(history.navigate(10, true), Some(3));
        assert_eq!(history.navigate(3, false), Some(10));
    }

    #[test]
    fn only_a_new_successful_jump_clears_forward_history() {
        let mut history = NavigationHistory::default();
        history.begin_jump(8);
        history.settle(&2);
        assert_eq!(history.navigate(2, false), Some(8));
        history.settle(&9); // Reading after Back keeps Forward.
        history.begin_jump(9);
        history.settle(&9); // Failed or same-position jump.
        assert_eq!(history.availability(), (false, true));
        history.begin_jump(9);
        history.settle(&4);
        assert_eq!(history.availability(), (true, false));
        assert_eq!(history.navigate(4, true), None);
        assert_eq!(history.navigate(4, false), Some(9));
    }

    #[test]
    fn rapid_jumps_retain_the_last_displayed_origin() {
        let mut history = NavigationHistory::default();
        history.begin_jump("chapter 8, paragraph 12");
        history.begin_jump("intermediate destination");
        history.settle(&"final destination");
        assert_eq!(history.navigate("final destination", false), Some("chapter 8, paragraph 12"));
        assert_eq!(history.navigate("chapter 8, paragraph 12", false), None);
    }

    #[test]
    fn back_skips_entries_already_reached_by_ordinary_reading() {
        let mut history = NavigationHistory::default();
        history.begin_jump(8);
        history.settle(&2);
        history.begin_jump(2);
        history.settle(&1);
        assert_eq!(history.navigate(2, false), Some(8));
        assert_eq!(history.navigate(8, true), Some(2));
        assert_eq!(history.availability(), (true, false));
    }
}
