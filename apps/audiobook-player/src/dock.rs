use super::*;
use gpui::EventEmitter;
use gpui_component::Disableable;
use gpui_component::button::ButtonVariants;

pub enum DockAction {
    Expand,
    Close,
}

/// Compact projection of the retained playback entity. It has no decoder or
/// streaming resources of its own.
pub struct AudiobookDock {
    player: Entity<PlaybackSession>,
    snapshot: (String, Option<String>, Option<String>, bool, Option<String>, bool, Option<u64>),
    speed_menu_open: bool,
    sleep_menu_open: bool,
}

impl AudiobookDock {
    pub fn new(player: Entity<PlaybackSession>, cx: &mut Context<Self>) -> Self {
        cx.observe(&player, |dock, player, cx| {
            let next = Self::snapshot(player.read(cx));
            if next != dock.snapshot {
                dock.snapshot = next;
                cx.notify();
            }
        })
        .detach();
        let snapshot = Self::snapshot(player.read(cx));
        Self { player, snapshot, speed_menu_open: false, sleep_menu_open: false }
    }

    fn snapshot(player: &PlaybackSession) -> (String, Option<String>, Option<String>, bool, Option<String>, bool, Option<u64>) {
        let title = player.book.as_ref().map(|book| book.title.clone()).unwrap_or_else(|| "Opening audiobook…".into());
        let subtitle = player.book.as_ref().and_then(|book| book.author.clone().or_else(|| book.narrator.clone()));
        let chapter = player.book.as_ref().and_then(|book| book.chapters.get(player.active_chapter_index())).map(|chapter| super::chapter_display_title(&chapter.title));
        (title, subtitle, chapter, player.transport_playing(), player.error.clone(), player.is_buffering(), player.book.as_ref().and_then(|book| book.cover.as_ref()).map(|cover| cover.id()))
    }
}

impl EventEmitter<DockAction> for AudiobookDock {}

impl super::PlaybackTarget for AudiobookDock {
    fn toggle_playback(&mut self, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| player.request_toggle(cx));
    }
    fn seek_by(&mut self, seconds: f64, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| {
            player.seek_by(seconds);
            cx.notify();
        });
    }
    fn previous_chapter(&mut self, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| {
            player.previous_chapter();
            cx.notify();
        });
    }
    fn next_chapter(&mut self, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| {
            player.next_chapter();
            cx.notify();
        });
    }
    fn set_rate(&mut self, rate: f64, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| {
            player.set_rate(rate, cx);
            cx.notify();
        });
        self.speed_menu_open = false;
    }
    fn set_sleep(&mut self, plan: Option<super::SleepPlan>, cx: &mut gpui::App) {
        self.player.update(cx, |player, cx| {
            player.set_sleep(plan);
            cx.notify();
        });
        self.sleep_menu_open = false;
    }
    fn toggle_sleep_menu(&mut self) {
        self.sleep_menu_open = !self.sleep_menu_open;
        self.speed_menu_open = false;
    }
    fn toggle_speed_menu(&mut self) {
        self.speed_menu_open = !self.speed_menu_open;
        self.sleep_menu_open = false;
    }
    fn active_chapter_index(&self, cx: &gpui::App) -> usize {
        self.player.read(cx).active_chapter_index()
    }
    fn sleep_label(&self, cx: &gpui::App) -> Option<gpui::SharedString> {
        self.player.read(cx).sleep_label()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DockHost {
        dock: Entity<AudiobookDock>,
        expanded: usize,
        closed: usize,
    }
    impl Render for DockHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().child(div().flex_1().min_h_0().debug_selector(|| "dock-test-browsing".into()).child("Browsing stays visible")).child(self.dock.clone())
        }
    }

    #[gpui::test]
    fn rendered_dock_is_compact_and_routes_body_and_close_separately(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let (sender, _) = async_channel::unbounded();
        let player = cx.new(|_| {
            let mut player = PlaybackSession::new(1.0, sender);
            player.stopped = true;
            player.book = Some(Arc::new(AudiobookBook {
                title: "A very long audiobook title that must not push the transport buttons outside the screen".into(),
                author: None,
                narrator: None,
                duration: Duration::from_secs(100),
                chapters: vec![],
                cover: None,
            }));
            player
        });
        let dock = cx.new(|cx| AudiobookDock::new(player, cx));
        let host = cx.new(|cx| {
            cx.subscribe(&dock, |host: &mut DockHost, _, action, cx| {
                match action {
                    DockAction::Expand => host.expanded += 1,
                    DockAction::Close => host.closed += 1,
                }
                cx.notify();
            })
            .detach();
            DockHost { dock, expanded: 0, closed: 0 }
        });
        let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(host.clone(), window, cx));
        for width in [320.0, 600.0, 1200.0] {
            cx.simulate_resize(gpui::size(px(width), px(800.0)));
            cx.run_until_parked();
            let dock = cx.debug_bounds("audiobook-dock").unwrap();
            let browsing = cx.debug_bounds("dock-test-browsing").unwrap();
            assert!(dock.size.height < px(150.0), "dock must use intrinsic rather than maximum height");
            assert!(browsing.size.height > px(600.0));
            assert!(browsing.bottom() <= dock.top());
            for selector in ["dock-play", "dock-skip", "dock-close"] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(control.left() >= dock.left() && control.right() <= dock.right());
            }
        }
        let expand = cx.debug_bounds("audiobook-dock-expand").unwrap();
        cx.simulate_click(expand.center(), gpui::Modifiers::none());
        let close = cx.debug_bounds("dock-close").unwrap();
        cx.simulate_click(close.center(), gpui::Modifiers::none());
        host.read_with(cx, |host, _| {
            assert_eq!(host.expanded, 1);
            assert_eq!(host.closed, 1);
        });
    }

    #[test]
    fn snapshot_tracks_cover_but_not_playback_clock() {
        let (sender, _) = async_channel::unbounded();
        let mut player = PlaybackSession::new(1.0, sender);
        player.stopped = true;
        player.book = Some(Arc::new(AudiobookBook { title: "Book".into(), author: None, narrator: None, duration: Duration::from_secs(100), chapters: vec![], cover: None }));
        let initial = AudiobookDock::snapshot(&player);
        player.position = Duration::from_secs(10);
        assert_eq!(initial, AudiobookDock::snapshot(&player));
        Arc::make_mut(player.book.as_mut().unwrap()).cover = Some(Arc::new(Image::from_bytes(ImageFormat::Jpeg, vec![0xff, 0xd8, 0xff])));
        assert_ne!(initial, AudiobookDock::snapshot(&player));
    }

    #[gpui::test]
    fn clock_notifications_do_not_invalidate_dock_but_metadata_changes_do(cx: &mut gpui::TestAppContext) {
        let (sender, _) = async_channel::unbounded();
        let player = cx.new(|_| {
            let mut player = PlaybackSession::new(1.0, sender);
            player.stopped = true;
            player.book = Some(Arc::new(AudiobookBook { title: "Book".into(), author: None, narrator: None, duration: Duration::from_secs(100), chapters: vec![], cover: None }));
            player
        });
        let dock = cx.new(|cx| AudiobookDock::new(player.clone(), cx));
        let notifications = Rc::new(Cell::new(0));
        let recorded = notifications.clone();
        let _observer = cx.new(|cx| {
            cx.observe(&dock, move |_: &mut (), _, _| recorded.set(recorded.get() + 1)).detach();
        });
        cx.run_until_parked();
        let before = notifications.get();
        for second in 1..=20 {
            player.update(cx, |player, cx| {
                player.position = Duration::from_secs(second);
                cx.notify();
            });
            cx.run_until_parked();
        }
        assert_eq!(notifications.get(), before);
        player.update(cx, |player, cx| {
            Arc::make_mut(player.book.as_mut().unwrap()).title = "Updated metadata".into();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(notifications.get(), before + 1);
    }
}

impl Render for AudiobookDock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let player = self.player.read(cx);
        let cover = player.book.as_ref().and_then(|book| book.cover.clone());
        let ready = player.can_play();
        let layout = super::MenuLayout::for_window(window);
        if layout.compact {
            let title = self.snapshot.0.clone();
            let subtitle = self.snapshot.1.clone();
            let chapter = self.snapshot.2.clone().unwrap_or_else(|| "No chapter".into());
            let expand = cx.entity();
            let mobile_cover = cover.map(|cover| img(cover).w(px(64.0)).h(px(80.0)).flex_none().object_fit(ObjectFit::Cover).into_any_element());
            let title_line = div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title).when_some(subtitle, |line, subtitle| line.child(": ").child(subtitle));
            let chapter_line = div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(theme.text_muted).child(chapter);
            let mobile_content =
                div().id("audiobook-dock-expand").flex_1().min_w_0().flex().flex_col().justify_center().cursor_pointer().on_click(move |_, _, cx| expand.update(cx, |_, cx| cx.emit(DockAction::Expand))).child(title_line).child(chapter_line);
            let mobile_play_pause =
                ui_components::base_button("dock-play").label(if self.snapshot.3 { "Pause" } else { "Play" }).ghost().disabled(!ready).on_click(cx.listener(|dock, _, _, cx| dock.player.update(cx, |player, cx| player.request_toggle(cx))));
            let duration = player.book.as_ref().map(|book| book.duration).unwrap_or_default();
            let position = player.position.min(duration);
            let progress_fraction = if duration.is_zero() { 0.0 } else { (position.as_secs_f64() / duration.as_secs_f64()) as f32 };
            let progress_track = div()
                .id("audiobook-dock-progress")
                .relative()
                .w_full()
                .h(px(16.0))
                .flex()
                .items_center()
                .child(div().relative().w_full().h(px(3.0)).bg(theme.rule).child(div().absolute().left_0().top_0().h_full().w(gpui::relative(progress_fraction)).bg(theme.accent)));
            let mobile_progress = div().w_full().px(px(8.0)).pb(px(8.0)).child(progress_track);
            let mobile_header = div().w_full().flex().items_center().gap(px(8.0)).px(px(8.0)).pt(px(8.0)).children(mobile_cover).child(mobile_content).child(mobile_play_pause);
            return div().id("audiobook-dock").occlude().w_full().flex_none().flex().flex_col().border_t_1().border_color(theme.rule).bg(theme.page_bg).text_color(theme.text).child(mobile_header).child(mobile_progress).into_any_element();
        }
        let desktop_cover = cover.map(|cover| img(cover).w(px(64.0)).h(px(80.0)).flex_none().object_fit(ObjectFit::Cover).into_any_element());
        let desktop_title = div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.snapshot.0.clone());
        let desktop_subtitle = self.snapshot.1.clone().map(|subtitle| div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(theme.text_muted).child(subtitle));
        let desktop_book = div()
            .id("audiobook-dock-expand")
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .justify_center()
            .cursor_pointer()
            .debug_selector(|| "audiobook-dock-expand".into())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DockAction::Expand)))
            .child(desktop_title)
            .children(desktop_subtitle);
        let desktop_book_area = div().flex_1().min_w_0().flex().items_center().gap(px(8.0)).children(desktop_cover).child(desktop_book);

        let target = cx.entity().downgrade();
        let previous_target = target.clone();
        let back_target = target.clone();
        let play_target = target.clone();
        let forward_target = target.clone();
        let next_target = target.clone();
        let desktop_controls = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(super::transport_control("dock-previous-chapter", super::chapter_icon(false, theme), theme).w(px(56.0)).flex_none().mx(px(2.0)).on_click(move |_, _, cx| {
                let _ = previous_target.update(cx, |dock, cx| {
                    dock.previous_chapter(cx);
                    cx.notify();
                });
            }))
            .child(super::transport_control("dock-seek-back", super::skip_icon(false, "15"), theme).w(px(56.0)).flex_none().mx(px(2.0)).on_click(move |_, _, cx| {
                let _ = back_target.update(cx, |dock, cx| {
                    dock.seek_by(super::SKIP_BACK_SECONDS, cx);
                    cx.notify();
                });
            }))
            .child(
                super::transport_control_base("dock-play", Icon::new(if self.snapshot.3 { IconName::Pause } else { IconName::Play }).size(px(22.0)), theme)
                    .w(px(56.0))
                    .flex_none()
                    .mx(px(2.0))
                    .bg(theme.accent)
                    .text_color(theme.accent_text)
                    .hover(|style| style.bg(theme.accent_hover))
                    .on_click(move |_, _, cx| {
                        let _ = play_target.update(cx, |dock, cx| {
                            dock.toggle_playback(cx);
                            cx.notify();
                        });
                    }),
            )
            .child(super::transport_control("dock-seek-forward", super::skip_icon(true, "30"), theme).w(px(56.0)).flex_none().mx(px(2.0)).on_click(move |_, _, cx| {
                let _ = forward_target.update(cx, |dock, cx| {
                    dock.seek_by(super::SKIP_FORWARD_SECONDS, cx);
                    cx.notify();
                });
            }))
            .child(super::transport_control("dock-next-chapter", super::chapter_icon(true, theme), theme).w(px(56.0)).flex_none().mx(px(2.0)).on_click(move |_, _, cx| {
                let _ = next_target.update(cx, |dock, cx| {
                    dock.next_chapter(cx);
                    cx.notify();
                });
            }));

        let sleep_target = target.clone();
        let sleep_label = player.sleep_label();
        let sleep_content = sleep_label.clone().map(|label| label.into_any_element()).unwrap_or_else(|| Icon::empty().path("icons/alarm.svg").size(px(18.0)).into_any_element());
        let sleep = div().relative().w(px(80.0)).flex_none().when(self.sleep_menu_open, |wrapper| wrapper.child(gpui::deferred(super::sleep_menu(target.clone(), sleep_label.clone(), layout, theme)).with_priority(1))).child(
            super::menu_trigger("dock-sleep-timer", sleep_content, self.sleep_menu_open || sleep_label.is_some(), theme).on_click(move |_, _, cx| {
                let _ = sleep_target.update(cx, |dock, cx| {
                    dock.toggle_sleep_menu();
                    cx.notify();
                });
            }),
        );
        let speed_target = target.clone();
        let speed = div().relative().w(px(80.0)).flex_none().when(self.speed_menu_open, |wrapper| wrapper.child(gpui::deferred(super::playback_speed_menu(speed_target.clone(), player.rate, layout, theme)).with_priority(1))).child(
            super::menu_trigger("dock-playback-rate", super::format_playback_rate(player.rate), self.speed_menu_open, theme).w(px(80.0)).flex_none().debug_selector(|| "dock-playback-rate".into()).on_click(move |_, _, cx| {
                let _ = speed_target.update(cx, |dock, cx| {
                    dock.toggle_speed_menu();
                    cx.notify();
                });
            }),
        );
        let desktop_tools =
            div().flex_none().flex().items_center().justify_end().gap(px(4.0)).child(sleep).child(speed).child(ui_components::base_button("dock-close").label("Close").ghost().on_click(cx.listener(|_, _, _, cx| cx.emit(DockAction::Close))));

        let (chapter_position, chapter_duration) = player
            .book
            .as_ref()
            .and_then(|book| book.chapters.get(player.active_chapter_index()))
            .map(|chapter| {
                let duration = chapter.end.saturating_sub(chapter.start);
                (player.position.saturating_sub(chapter.start).min(duration), duration)
            })
            .unwrap_or_default();
        let progress_fraction = if chapter_duration.is_zero() { 0.0 } else { (chapter_position.as_secs_f64() / chapter_duration.as_secs_f64()) as f32 };
        let current_chapter = self.snapshot.2.clone().unwrap_or_else(|| "No chapter".into());
        let progress_track = div()
            .id("audiobook-dock-progress")
            .relative()
            .w_full()
            .h(px(16.0))
            .flex()
            .items_center()
            .child(div().relative().w_full().h(px(3.0)).bg(theme.rule).child(div().absolute().left_0().top_0().h_full().w(gpui::relative(progress_fraction)).bg(theme.accent)));
        let progress_meta = div()
            .w_full()
            .flex()
            .items_center()
            .text_size(gpui::rems(ui_components::TEXT_XS))
            .text_color(theme.text_muted)
            .child(div().w(px(56.0)).flex_none().child(super::format_time(chapter_position)))
            .child(div().flex_1().min_w_0().text_center().overflow_hidden().whitespace_nowrap().text_ellipsis().child(current_chapter))
            .child(div().w(px(56.0)).flex_none().text_right().child(super::format_time(chapter_duration)));
        let desktop_progress = div().w_full().flex().flex_col().child(progress_track).child(progress_meta);
        let desktop_header = div().w_full().flex().items_center().gap(px(8.0)).child(desktop_book_area).child(desktop_controls).child(desktop_tools);
        div()
            .id("audiobook-dock")
            .occlude()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(8.0))
            .debug_selector(|| "audiobook-dock".into())
            .border_t_1()
            .border_color(theme.rule)
            .bg(theme.page_bg)
            .text_color(theme.text)
            .child(desktop_header)
            .child(div().w_full().flex().justify_center().child(desktop_progress))
            .into_any_element()
    }
}
