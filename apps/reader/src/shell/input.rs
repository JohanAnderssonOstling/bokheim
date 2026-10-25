//! Format-neutral dispatch for reader input commands.

use gpui::{App, Context, KeyBinding, Window};

use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, NextSidebarTab, PreviousLine, PreviousPage, PreviousSidebarTab, ReturnToLibrary, ToggleSearch, ToggleSidebar, ToggleToc};

/// A reader implements this only for commands it supports.
pub(crate) trait CommandTarget<C>: Sized + 'static {
    fn execute(&mut self, command: &C, window: &mut Window, cx: &mut Context<Self>);
}

/// Commands every reader surface supports.
pub(crate) trait CommonReaderCommands:
    CommandTarget<PreviousPage>
    + CommandTarget<NextPage>
    + CommandTarget<PreviousLine>
    + CommandTarget<NextLine>
    + CommandTarget<NextSidebarTab>
    + CommandTarget<PreviousSidebarTab>
    + CommandTarget<ToggleSidebar>
    + CommandTarget<ToggleToc>
    + CommandTarget<FocusReaderSearch>
    + CommandTarget<ToggleSearch>
    + CommandTarget<CloseSearch>
    + CommandTarget<IncreaseScale>
    + CommandTarget<DecreaseScale>
    + CommandTarget<ReturnToLibrary>
{
}

impl<T> CommonReaderCommands for T where
    T: CommandTarget<PreviousPage>
        + CommandTarget<NextPage>
        + CommandTarget<PreviousLine>
        + CommandTarget<NextLine>
        + CommandTarget<NextSidebarTab>
        + CommandTarget<PreviousSidebarTab>
        + CommandTarget<ToggleSidebar>
        + CommandTarget<ToggleToc>
        + CommandTarget<FocusReaderSearch>
        + CommandTarget<ToggleSearch>
        + CommandTarget<CloseSearch>
        + CommandTarget<IncreaseScale>
        + CommandTarget<DecreaseScale>
        + CommandTarget<ReturnToLibrary>
{
}

/// Adapts a GPUI action listener to its format-specific command implementation.
pub(crate) fn dispatch<R, C>(reader: &mut R, command: &C, window: &mut Window, cx: &mut Context<R>)
where
    R: CommandTarget<C>,
    C: 'static,
{
    reader.execute(command, window, cx);
}

/// Binds commands handled by the reader shell rather than the document.
pub(crate) fn bind_reader_keys(cx: &mut App) {
    use ui_components::{ShortcutPage::Reader, bind_shortcut};
    bind_shortcut(cx, Reader, "reader.next_tab", "Next sidebar tab", &["ctrl-tab"], NextSidebarTab, "Reader");
    bind_shortcut(cx, Reader, "reader.previous_tab", "Previous sidebar tab", &["ctrl-shift-tab"], PreviousSidebarTab, "Reader");
    bind_shortcut(cx, Reader, "reader.sidebar", "Toggle sidebar", &["secondary-s"], ToggleSidebar, "Reader");
    bind_shortcut(cx, Reader, "reader.contents", "Toggle contents", &["secondary-t"], ToggleToc, "Reader");
    bind_shortcut(cx, Reader, "reader.search", "Search in book", &["secondary-f"], FocusReaderSearch, "Reader");
    bind_shortcut(cx, Reader, "reader.close_search", "Close search or sidebar", &["escape"], CloseSearch, "Reader");
    bind_shortcut(cx, Reader, "reader.library", "Return to library", &["backspace"], ReturnToLibrary, "Reader");
    cx.bind_keys([
        #[cfg(not(target_os = "android"))]
        KeyBinding::new("back", ReturnToLibrary, Some("Reader")),
        #[cfg(target_os = "android")]
        KeyBinding::new("f24", ToggleToc, None),
    ]);
}

/// Handle panel shortcuts before nested inputs or sidebar navigation consume them.
pub(crate) fn install_reader_shortcuts<R: CommandTarget<FocusReaderSearch> + CommandTarget<ToggleToc>>(window: &mut Window, cx: &mut Context<R>) {
    let target = cx.entity().downgrade();
    let window_handle = window.window_handle();
    window.use_keyed_state(("reader-panel-keyboard", cx.entity_id()), cx, move |_, cx| {
        cx.intercept_keystrokes(move |event, window, cx| {
            let key = &event.keystroke;
            if window.window_handle() != window_handle || !event.context_stack.iter().any(|context| context.contains("Reader")) || event.context_stack.iter().any(|context| context.contains("ShortcutSettings")) {
                return;
            }
            let contents = ui_components::matches_shortcut("reader.contents", key, cx);
            let search = ui_components::matches_shortcut("reader.search", key, cx);
            if !contents && !search {
                return;
            }
            if let Some(target) = target.upgrade() {
                target.update(cx, |reader, cx| {
                    if contents {
                        reader.execute(&ToggleToc, window, cx);
                    } else {
                        reader.execute(&FocusReaderSearch, window, cx);
                    }
                });
                cx.stop_propagation();
            }
        })
    });
}

/// Binds navigation shared by reflowable and fixed-layout documents.
pub(crate) fn bind_document_keys(cx: &mut App, context: &'static str) {
    use ui_components::{ShortcutPage::Reader, bind_shortcut};
    bind_shortcut(cx, Reader, "reader.previous_line", "Previous line", &["up"], PreviousLine, context);
    bind_shortcut(cx, Reader, "reader.next_line", "Next line", &["down"], NextLine, context);
    bind_shortcut(cx, Reader, "reader.previous_page", "Previous page", &["left", "pageup"], PreviousPage, context);
    bind_shortcut(cx, Reader, "reader.next_page", "Next page", &["right", "pagedown", "space"], NextPage, context);
}
