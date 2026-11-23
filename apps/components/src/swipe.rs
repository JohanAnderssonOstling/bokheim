pub use gpui::PageTurnFrame;
#[cfg(not(feature = "kobo"))]
pub use gpui::{PageTurn, SwipePreview};

#[cfg(feature = "kobo")]
pub use disabled::{PageTurn, SwipePreview};

#[cfg(feature = "kobo")]
mod disabled {
    use gpui::{App, Axis, Pixels, Point, Size, Window};

    #[derive(Default)]
    pub struct SwipePreview;

    impl SwipePreview {
        pub fn drag(&mut self, _travel: Point<f32>, _window: &Window) -> bool {
            false
        }
        pub fn drag_in_viewport(&mut self, _travel: Point<f32>, _viewport: Size<Pixels>) -> bool {
            false
        }
        pub fn take_offset(&mut self) -> Point<Pixels> {
            Point::default()
        }
        pub fn settle(&mut self) {}
        pub fn clear(&mut self) {}
        pub fn offset(&mut self, _window: &mut Window, _cx: &App) -> Point<Pixels> {
            Point::default()
        }
    }

    pub struct PageTurn<S> {
        pub outgoing: S,
    }

    impl<S> PageTurn<S> {
        pub fn enabled(_cx: &App) -> bool {
            false
        }
        pub fn new(_outgoing: S, _forward: bool, _drag: Point<Pixels>, _viewport: Size<Pixels>, _cx: &App) -> Option<Self> {
            None
        }
        pub fn new_on_axis(_outgoing: S, _forward: bool, _drag: Point<Pixels>, _viewport: Size<Pixels>, _axis: Axis, _cx: &App) -> Option<Self> {
            None
        }
        pub fn frame(&mut self, _viewport: Size<Pixels>, _ready: bool, _window: &mut Window, _cx: &App) -> Option<super::PageTurnFrame> {
            None
        }
    }
}

#[cfg(all(test, feature = "kobo"))]
mod tests {
    use super::*;
    use gpui::{Pixels, Point, TestAppContext, point};
    #[gpui::test]
    fn kobo_never_starts_page_or_settle_animations(cx: &mut TestAppContext) {
        let (_, visual) = cx.add_window_view(|_, _| gpui::Empty);
        visual.update(|window, cx| {
            let mut preview = SwipePreview::default();
            assert!(!preview.drag(point(-120.0, 0.0), window));
            preview.settle();
            assert_eq!(preview.offset(window, cx), Point::default());
            assert_eq!(preview.take_offset(), Point::default());
            assert!(PageTurn::new((), true, Point::default(), window.viewport_size(), cx).is_none());
        });
    }
}
