use floem::prelude::{h_stack, v_stack, Scroll, ScrollExt, Stack, ViewTuple};
use floem::views::stack_from_iter;
use floem::IntoView;

pub fn create_button_group<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children)
}

pub fn create_top_row(content: Stack) -> Stack {
    h_stack((content,))
}

pub fn create_list_header<VT: ViewTuple + 'static>(children: VT) -> Stack {
    h_stack(children)
}

pub fn create_list_bodyr<V>(iterator: impl IntoIterator<Item = V>) -> Scroll
where
    V: IntoView + 'static,
{
    stack_from_iter(iterator).scroll()
}

pub fn create_list<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}

pub fn create_centered_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}

pub fn create_full_stack<VT: ViewTuple + 'static>(children: VT) -> Stack {
    v_stack(children)
}
