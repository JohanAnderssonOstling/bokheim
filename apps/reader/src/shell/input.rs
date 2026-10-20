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
    cx.bind_keys([
        KeyBinding::new("tab", NextSidebarTab, Some("Reader")),
        KeyBinding::new("shift-tab", PreviousSidebarTab, Some("Reader")),
        KeyBinding::new("ctrl-tab", NextSidebarTab, Some("Reader")),
        KeyBinding::new("ctrl-shift-tab", PreviousSidebarTab, Some("Reader")),
        KeyBinding::new("secondary-s", ToggleSidebar, Some("Reader")),
        KeyBinding::new("secondary-t", ToggleToc, Some("Reader")),
        KeyBinding::new("secondary-f", FocusReaderSearch, Some("Reader")),
        KeyBinding::new("escape", CloseSearch, Some("Reader")),
        // Backspace leaves the book the same way the toolbar's library button
        // does, so the keyboard and the chrome cannot drift apart. The OS back
        // button/gesture does the same on native mobile, where the on-screen
        // back button is hidden.
        KeyBinding::new("backspace", ReturnToLibrary, Some("Reader")),
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
            if window.window_handle() != window_handle || !event.context_stack.iter().any(|context| context.contains("Reader")) || !matches!(key.key.as_str(), "f" | "t") || !ui_components::primary_modifier_only(key.modifiers) {
                return;
            }
            if let Some(target) = target.upgrade() {
                target.update(cx, |reader, cx| match key.key.as_str() {
                    "t" => reader.execute(&ToggleToc, window, cx),
                    _ => reader.execute(&FocusReaderSearch, window, cx),
                });
                cx.stop_propagation();
            }
        })
    });
}

/// Binds navigation shared by reflowable and fixed-layout documents.
pub(crate) fn bind_document_keys(cx: &mut App, context: &'static str) {
    cx.bind_keys([
        KeyBinding::new("up", PreviousLine, Some(context)),
        KeyBinding::new("down", NextLine, Some(context)),
        KeyBinding::new("left", PreviousPage, Some(context)),
        KeyBinding::new("right", NextPage, Some(context)),
        KeyBinding::new("pageup", PreviousPage, Some(context)),
        KeyBinding::new("pagedown", NextPage, Some(context)),
        KeyBinding::new("space", NextPage, Some(context)),
        KeyBinding::new("secondary-=", IncreaseScale, Some(context)),
        KeyBinding::new("secondary-+", IncreaseScale, Some(context)),
        KeyBinding::new("secondary--", DecreaseScale, Some(context)),
    ]);
}
