//! Format-neutral dispatch for reader input commands.

use std::time::Duration;

use gpui::{App, Context, Focusable, KeyBinding, Task, Window};

use super::{
    chrome::{self, ChromeState},
    state_panel::{SidebarState, SidebarTab, adjacent_available_tab},
    toolbar::SidebarTarget,
};

use crate::{
    CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NavigateBack, NavigateForward, NextLine, NextPage, NextSidebarTab, OpenReaderSettings, PreviousLine, PreviousPage, PreviousSidebarTab, ReturnToLibrary, RevealReaderChrome,
    ToggleNotes, ToggleSearch, ToggleSidebar, ToggleToc, ToggleTts,
};

/// A reader implements this only for commands it supports.
pub(crate) trait CommandTarget<C>: Sized + 'static {
    fn execute(&mut self, command: &C, window: &mut Window, cx: &mut Context<Self>);
}

/// Run one navigation action after a physical wheel burst has been quiet for 150 ms.
pub(crate) fn debounce_wheel<T: 'static>(task: &mut Option<Task<()>>, window: &mut Window, cx: &mut Context<T>, action: impl FnOnce(&mut T, &mut Window, &mut Context<T>) + 'static) {
    task.take();
    let timer = cx.background_executor().timer(Duration::from_millis(150));
    *task = Some(cx.spawn_in(window, async move |this, cx| {
        timer.await;
        let _ = this.update_in(cx, action);
    }));
}

/// Sidebar commands whose policy is independent of a document's renderer.
/// Format-specific panel selection and close hooks stay on `SidebarTarget`.
pub(crate) trait ReaderSidebarCommands: SidebarTarget + Focusable {
    fn sidebar_controls(&mut self) -> (&mut SidebarState, &mut ChromeState);

    fn hide_sidebar_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sidebar, chrome) = self.sidebar_controls();
        if ui_components::uses_mobile_reader_controls(window) {
            sidebar.close(cx);
        } else {
            chrome.hide(cx);
        }
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn cycle_sidebar_controls(&mut self, direction: i32, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.sidebar_tab();
        let tabs = self.available_tabs();
        let (sidebar, chrome) = self.sidebar_controls();
        let visible = chrome.is_visible() && (!ui_components::uses_mobile_reader_controls(window) || sidebar.is_visible());
        let tab = if visible { adjacent_available_tab(active, tabs, direction) } else { active };
        chrome.show(cx);
        self.keyboard_visibility().set(!ui_components::uses_mobile_reader_controls(window));
        self.select_sidebar_tab(tab, window, cx);
    }

    fn toggle_sidebar_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sidebar, controls) = self.sidebar_controls();
        let visible = controls.is_visible() && (!chrome::sidebar_overlays_document(window) || sidebar.is_visible());
        if visible {
            sidebar.close(cx);
            controls.hide(cx);
            self.focus_handle(cx).focus(window, cx);
        } else {
            sidebar.reopen(cx);
            controls.show(cx);
            self.focus_sidebar_body(window, cx);
        }
        cx.notify();
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<NextSidebarTab> for R {
    fn execute(&mut self, _: &NextSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_sidebar_controls(1, window, cx);
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<PreviousSidebarTab> for R {
    fn execute(&mut self, _: &PreviousSidebarTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_sidebar_controls(-1, window, cx);
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<ToggleSidebar> for R {
    fn execute(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sidebar_controls(window, cx);
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<RevealReaderChrome> for R {
    fn execute(&mut self, _: &RevealReaderChrome, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_controls().1.show(cx);
    }
}

impl<R: SidebarTarget> CommandTarget<OpenReaderSettings> for R {
    fn execute(&mut self, _: &OpenReaderSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_visibility().set(!ui_components::uses_mobile_reader_controls(window));
        self.select_sidebar_tab(SidebarTab::Settings, window, cx);
    }
}

impl<R: SidebarTarget> CommandTarget<ToggleNotes> for R {
    fn execute(&mut self, _: &ToggleNotes, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sidebar_tab(SidebarTab::Notes, window, cx);
    }
}

impl<R: SidebarTarget> CommandTarget<ToggleTts> for R {
    fn execute(&mut self, _: &ToggleTts, window: &mut Window, cx: &mut Context<Self>) {
        self.press_tts(window, cx);
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<CloseSearch> for R {
    fn execute(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_sidebar(window, cx);
    }
}

impl<R: ReaderSidebarCommands> CommandTarget<ReturnToLibrary> for R {
    fn execute(&mut self, _: &ReturnToLibrary, window: &mut Window, cx: &mut Context<Self>) {
        self.close_reader(window, cx);
    }
}

impl<R: SidebarTarget> CommandTarget<NavigateBack> for R {
    fn execute(&mut self, _: &NavigateBack, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_history(false, window, cx);
    }
}

impl<R: SidebarTarget> CommandTarget<NavigateForward> for R {
    fn execute(&mut self, _: &NavigateForward, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_history(true, window, cx);
    }
}

/// Commands every reader surface supports.
pub(crate) trait CommonReaderCommands:
    CommandTarget<NavigateBack>
    + CommandTarget<NavigateForward>
    + CommandTarget<PreviousPage>
    + CommandTarget<NextPage>
    + CommandTarget<PreviousLine>
    + CommandTarget<NextLine>
    + CommandTarget<NextSidebarTab>
    + CommandTarget<PreviousSidebarTab>
    + CommandTarget<ToggleSidebar>
    + CommandTarget<ToggleToc>
    + CommandTarget<RevealReaderChrome>
    + CommandTarget<ToggleNotes>
    + CommandTarget<ToggleTts>
    + CommandTarget<FocusReaderSearch>
    + CommandTarget<OpenReaderSettings>
    + CommandTarget<ToggleSearch>
    + CommandTarget<CloseSearch>
    + CommandTarget<IncreaseScale>
    + CommandTarget<DecreaseScale>
    + CommandTarget<ReturnToLibrary>
{
}

impl<T> CommonReaderCommands for T where
    T: CommandTarget<NavigateBack>
        + CommandTarget<NavigateForward>
        + CommandTarget<PreviousPage>
        + CommandTarget<NextPage>
        + CommandTarget<PreviousLine>
        + CommandTarget<NextLine>
        + CommandTarget<NextSidebarTab>
        + CommandTarget<PreviousSidebarTab>
        + CommandTarget<ToggleSidebar>
        + CommandTarget<ToggleToc>
        + CommandTarget<RevealReaderChrome>
        + CommandTarget<ToggleNotes>
        + CommandTarget<ToggleTts>
        + CommandTarget<FocusReaderSearch>
        + CommandTarget<OpenReaderSettings>
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
    bind_shortcut(cx, Reader, "reader.back", "Back in reading history", &["alt-left"], NavigateBack, "Reader");
    bind_shortcut(cx, Reader, "reader.forward", "Forward in reading history", &["alt-right"], NavigateForward, "Reader");
    bind_shortcut(cx, Reader, "reader.next_tab", "Next sidebar tab", &["ctrl-tab"], NextSidebarTab, "Reader");
    bind_shortcut(cx, Reader, "reader.previous_tab", "Previous sidebar tab", &["ctrl-shift-tab"], PreviousSidebarTab, "Reader");
    bind_shortcut(cx, Reader, "reader.settings", "Open reader settings", &["secondary-s"], OpenReaderSettings, "Reader");
    bind_shortcut(cx, Reader, "reader.contents", "Toggle contents", &["secondary-shift-t"], ToggleToc, "Reader");
    bind_shortcut(cx, Reader, "reader.tts", "Toggle text-to-speech", &["ctrl-t"], ToggleTts, "Reader");
    bind_shortcut(cx, Reader, "reader.notes", "Toggle notes", &["ctrl-n"], ToggleNotes, "Reader");
    bind_shortcut(cx, Reader, "reader.search", "Search in book", &["secondary-f"], FocusReaderSearch, "Reader");
    bind_shortcut(cx, Reader, "reader.close_search", "Close search or sidebar", &["escape"], CloseSearch, "Reader");
    bind_shortcut(cx, Reader, "reader.library", "Return to library", &["backspace"], ReturnToLibrary, "Reader");
    cx.bind_keys([
        #[cfg(not(target_os = "android"))]
        KeyBinding::new("back", ReturnToLibrary, Some("Reader")),
        #[cfg(target_os = "android")]
        KeyBinding::new("f24", RevealReaderChrome, Some("Reader")),
    ]);
}

/// Handle panel shortcuts before nested inputs or sidebar navigation consume them.
pub(crate) fn install_reader_shortcuts<
    R: CommandTarget<NavigateBack> + CommandTarget<NavigateForward> + CommandTarget<FocusReaderSearch> + CommandTarget<ToggleToc> + CommandTarget<OpenReaderSettings> + CommandTarget<ToggleNotes> + CommandTarget<ToggleTts>,
>(
    window: &mut Window, cx: &mut Context<R>,
) {
    let target = cx.entity().downgrade();
    let window_handle = window.window_handle();
    window.use_keyed_state(("reader-panel-keyboard", cx.entity_id()), cx, move |_, cx| {
        cx.intercept_keystrokes(move |event, window, cx| {
            let key = &event.keystroke;
            if window.window_handle() != window_handle
                || !event.context_stack.iter().any(|context| context.contains("Reader"))
                || event.context_stack.iter().any(|context| context.contains("ShortcutSettings") || context.contains("TtsVoicePicker"))
            {
                return;
            }
            let back = ui_components::matches_shortcut("reader.back", key, cx);
            let forward = ui_components::matches_shortcut("reader.forward", key, cx);
            let contents = ui_components::matches_shortcut("reader.contents", key, cx);
            let search = ui_components::matches_shortcut("reader.search", key, cx);
            let settings = ui_components::matches_shortcut("reader.settings", key, cx);
            let notes = ui_components::matches_shortcut("reader.notes", key, cx);
            let tts = ui_components::matches_shortcut("reader.tts", key, cx);
            if !back && !forward && !contents && !search && !settings && !notes && !tts {
                return;
            }
            if let Some(target) = target.upgrade() {
                target.update(cx, |reader, cx| {
                    if back {
                        reader.execute(&NavigateBack, window, cx);
                    } else if forward {
                        reader.execute(&NavigateForward, window, cx);
                    } else if contents {
                        reader.execute(&ToggleToc, window, cx);
                    } else if search {
                        reader.execute(&FocusReaderSearch, window, cx);
                    } else if settings {
                        reader.execute(&OpenReaderSettings, window, cx);
                    } else if notes {
                        reader.execute(&ToggleNotes, window, cx);
                    } else {
                        reader.execute(&ToggleTts, window, cx);
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
