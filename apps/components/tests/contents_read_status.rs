use std::{cell::Cell, rc::Rc};

use gpui::{Context, Div, Modifiers, MouseButton, Render, TestAppContext, Window, div, point, prelude::*, px};
use gpui_component::Root;
use ui_components::{BrowserTheme, ContentsReadReset, ContentsRow, contents_row};

struct ContentsFixture {
    resets: Rc<Cell<usize>>,
    navigations: Rc<Cell<usize>>,
    parent_mouse_downs: Rc<Cell<usize>>,
}

impl Render for ContentsFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let resets = self.resets.clone();
        let navigations = self.navigations.clone();
        let parent_mouse_downs = self.parent_mouse_downs.clone();
        let reset: ContentsReadReset = Rc::new(move |_, _, _| resets.set(resets.get() + 1));
        div().w(px(400.0)).child(
            contents_row("chapter-five", ContentsRow { read: true, ..Default::default() }, None::<Div>, "Chapter five", None, Some(reset), BrowserTheme::default())
                .debug_selector(|| "test-contents-row".into())
                .on_mouse_down(MouseButton::Left, move |_, _, _| parent_mouse_downs.set(parent_mouse_downs.get() + 1))
                .on_click(move |_, _, _| navigations.set(navigations.get() + 1)),
        )
    }
}

#[gpui::test]
fn checkmark_resets_without_navigating_or_toggling_the_parent(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let resets = Rc::new(Cell::new(0));
    let navigations = Rc::new(Cell::new(0));
    let parent_mouse_downs = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view(|window, cx| {
        let content = cx.new(|_| ContentsFixture { resets: resets.clone(), navigations: navigations.clone(), parent_mouse_downs: parent_mouse_downs.clone() });
        Root::new(content, window, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let checkmark = visual.debug_bounds("chapter-read-checkmark").unwrap();
    visual.simulate_click(checkmark.center(), Modifiers::default());
    assert_eq!(resets.get(), 1);
    assert_eq!(navigations.get(), 0);
    assert_eq!(parent_mouse_downs.get(), 0);
    let row = visual.debug_bounds("test-contents-row").unwrap();
    visual.simulate_click(point(row.left() + px(40.0), row.center().y), Modifiers::default());
    assert_eq!(navigations.get(), 1);
    assert_eq!(resets.get(), 1);
}
