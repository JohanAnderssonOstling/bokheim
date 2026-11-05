/// Accumulated finger travel. Panning an image consumes its swipe, and gestures
/// beginning at the system edge never navigate. Preview and turn policies stay
/// with the reader, including its threshold and reading direction.
#[derive(Default)]
pub struct PageSwipe {
    travel: Option<(f32, f32)>,
    ignored: bool,
    panned: bool,
}

impl PageSwipe {
    pub fn is_active(&self) -> bool {
        self.travel.is_some()
    }

    pub fn begin(&mut self, ignored: bool) {
        *self = Self { travel: Some((0.0, 0.0)), ignored, panned: false };
    }

    pub fn push(&mut self, dx: f32, dy: f32) -> Option<(f32, f32)> {
        let travel = self.travel.as_mut()?;
        if !dx.is_finite() || !dy.is_finite() {
            return None;
        }
        travel.0 += dx;
        travel.1 += dy;
        (!self.ignored).then_some(*travel)
    }

    pub fn mark_panned(&mut self) {
        self.panned = true;
    }

    pub fn finish(&mut self) -> Option<(f32, f32)> {
        let swipe = std::mem::take(self);
        swipe.travel.filter(|_| !swipe.ignored && !swipe.panned)
    }

    pub fn cancel(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_consumes_accumulated_travel_once() {
        let mut swipe = PageSwipe::default();
        assert_eq!(swipe.push(-20.0, 0.0), None);
        swipe.begin(false);
        assert_eq!(swipe.push(-20.0, 5.0), Some((-20.0, 5.0)));
        assert_eq!(swipe.push(-30.0, 5.0), Some((-50.0, 10.0)));
        assert_eq!(swipe.push(f32::NAN, 0.0), None);
        assert_eq!(swipe.finish(), Some((-50.0, 10.0)));
        assert_eq!(swipe.finish(), None);
        assert!(!swipe.is_active());
    }

    #[test]
    fn panning_cancellation_and_system_gestures_do_not_turn_pages() {
        let mut swipe = PageSwipe::default();
        swipe.begin(true);
        assert_eq!(swipe.push(0.0, -100.0), None);
        assert_eq!(swipe.finish(), None);
        swipe.begin(false);
        swipe.push(0.0, -100.0);
        swipe.mark_panned();
        assert_eq!(swipe.finish(), None);
        swipe.begin(false);
        swipe.push(0.0, -100.0);
        swipe.cancel();
        assert_eq!(swipe.finish(), None);
    }
}
