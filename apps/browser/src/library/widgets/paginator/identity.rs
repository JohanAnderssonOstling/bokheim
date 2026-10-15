use gpui::SharedString;

#[derive(Clone)]
pub(super) struct ItemPosition {
    pub(super) id: Option<SharedString>,
    pub(super) index: usize,
}

#[derive(Clone)]
pub(crate) struct PaginatorPosition {
    pub(super) cursor: Option<ItemPosition>,
    pub(super) anchor: Option<ItemPosition>,
    pub(super) selection: Vec<SharedString>,
    pub(super) keyboard_selection_active: bool,
}
