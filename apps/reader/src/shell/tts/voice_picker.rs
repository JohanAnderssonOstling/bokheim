//! The same installed/offline voice choices in a desktop popup and a mobile sheet.
use ui_text::text;

use super::*;
use gpui::{Anchor, Entity, FocusHandle, Focusable, Render, Subscription, WeakEntity, Window, px};
use gpui_component::{Selectable, WindowExt, popover::Popover};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Device(String),
    Bundled(NarrationVoice),
}
impl Choice {
    fn preference(&self) -> NarrationVoicePreference {
        match self {
            Self::Device(id) => NarrationVoicePreference { voice: NarrationVoice::Device, device_voice: Some(id.clone()) },
            Self::Bundled(voice) => NarrationVoicePreference { voice: *voice, device_voice: None },
        }
    }
}

#[derive(Clone, PartialEq)]
struct Item {
    choice: Choice,
    name: String,
    language: String,
    detail: Option<String>,
}
impl Item {
    fn sort_key(&self) -> (String, String, String, String) {
        (reader_speech::language_base(&self.language), self.name.to_lowercase(), self.name.clone(), format!("{:?}", self.choice))
    }
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    device: Vec<Item>,
    bundled: Vec<Item>,
    selected: Choice,
    preferences: BTreeMap<String, NarrationVoicePreference>,
    loading: bool,
    error: Option<String>,
}
impl Snapshot {
    fn is_selected(&self, item: &Item) -> bool {
        self.preferences.get(&reader_speech::language_base(&item.language)).map_or(self.selected == item.choice, |preference| *preference == item.choice.preference())
    }

    fn toggle(&self, item: &Item, state: &mut Narration) -> bool {
        if self.is_selected(item) {
            let remaining = self.device.iter().chain(&self.bundled).filter(|candidate| candidate.choice != item.choice && self.is_selected(candidate)).collect::<Vec<_>>();
            let Some(replacement) = remaining.first() else {
                return false;
            };
            for candidate in &remaining {
                state.voice_preferences.entry(reader_speech::language_base(&candidate.language)).or_insert_with(|| candidate.choice.preference());
            }
            state.voice_preferences.remove(&reader_speech::language_base(&item.language));
            if self.selected == item.choice {
                let preference = replacement.choice.preference();
                state.select_voice(preference.voice, preference.device_voice);
            }
        } else {
            let preference = item.choice.preference();
            state.remember_voice(&item.language, preference.voice, preference.device_voice);
        }
        true
    }

    fn new(state: &Narration) -> Self {
        let mut device = state.matching_voices().into_iter().map(|voice| Item { choice: Choice::Device(voice.id), name: voice.name, language: voice.language, detail: None }).collect::<Vec<_>>();
        device.sort_by_cached_key(Item::sort_key);
        let mut bundled = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        if state.languages.iter().all(|language| reader_speech::language_base(language).is_empty()) || supports_languages(&state.languages) {
            bundled.push(Item { choice: Choice::Bundled(NarrationVoice::Alba), name: text!(reader::alba).into(), language: "en".into(), detail: Some(text!(reader::scottish_english).into()) });
            #[cfg(not(target_os = "android"))]
            bundled.push(Item { choice: Choice::Bundled(NarrationVoice::George), name: text!(reader::george).into(), language: "en".into(), detail: Some(text!(reader::british_english).into()) });
        }
        bundled.sort_by_cached_key(Item::sort_key);
        Self {
            device,
            bundled,
            selected: if state.voice == NarrationVoice::Device { Choice::Device(state.device_voice.clone().unwrap_or_default()) } else { Choice::Bundled(state.voice) },
            preferences: state.voice_preferences.clone(),
            loading: state.voices_loading,
            error: state.voices_error.clone(),
        }
    }
}

type Close = Rc<dyn Fn(&mut Window, &mut App)>;
struct Picker<C: TtsReader> {
    owner: WeakEntity<C>,
    snapshot: Snapshot,
    device: bool,
    focus: FocusHandle,
    _observe: Subscription,
    close: Close,
}
impl<C: TtsReader> Picker<C> {
    fn new(owner: &Entity<C>, snapshot: Snapshot, close: Close, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let device = matches!(snapshot.selected, Choice::Device(_));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let observe = cx.observe(owner, |picker, owner, cx| {
            let snapshot = Snapshot::new(owner.read(cx).narration());
            if picker.snapshot != snapshot {
                picker.snapshot = snapshot;
                cx.notify();
            }
        });
        Self { owner: owner.downgrade(), snapshot, device, focus, _observe: observe, close }
    }
    fn select(&mut self, item: &Item, cx: &mut Context<Self>) {
        let saved = crate::settings::ReaderSettings::preferences(cx).narration_voices.clone();
        let _ = self.owner.update(cx, |reader, cx| {
            let state = reader.narration_mut();
            let was_active = state.is_active();
            state.voice_preferences.extend(saved);
            let snapshot = Snapshot::new(state);
            if !snapshot.toggle(item, state) {
                return;
            }
            let voice = state.voice;
            let id = state.device_voice.clone();
            let selections = state.voice_preferences.clone();
            if was_active && !state.is_active() {
                reader.stop_tts(cx);
            }
            crate::settings::ReaderSettings::update(cx, |preferences| {
                preferences.narration_voice = voice;
                preferences.narration_device_voice = id;
                preferences.narration_voices = selections;
            });
            cx.notify();
        });
        cx.notify();
    }
}
impl<C: TtsReader> Focusable for Picker<C> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl<C: TtsReader> Render for Picker<C> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let tabs = components::row(0.0)
            .child(
                components::base_button("tts-device-tab")
                    .debug_selector(|| "tts-device-tab".into())
                    .ghost()
                    .no_fill()
                    .label(text!(reader::device))
                    .flex_1()
                    .border_b_2()
                    .border_color(if self.device { theme.accent } else { theme.rule })
                    .on_click(cx.listener(|picker, _, _, cx| {
                        picker.device = true;
                        cx.notify();
                    })),
            )
            .child(
                components::base_button("tts-bokheim-tab")
                    .debug_selector(|| "tts-bokheim-tab".into())
                    .ghost()
                    .no_fill()
                    .label(text!(common::bokheim))
                    .flex_1()
                    .border_b_2()
                    .border_color(if self.device { theme.rule } else { theme.accent })
                    .on_click(cx.listener(|picker, _, _, cx| {
                        picker.device = false;
                        cx.notify();
                    })),
            )
            .child(components::outlined_icon_button("tts-voice-picker-close", text!(reader::close_voice_choices), IconName::Close).debug_selector(|| "tts-voice-picker-close".into()).on_click(cx.listener(|picker, _, window, cx| {
                (picker.close)(window, cx);
            })));
        let choices = if self.device { &self.snapshot.device } else { &self.snapshot.bundled };
        let rows = choices
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let choice = item.clone();
                let checked = self.snapshot.is_selected(item);
                components::row(0.0).w_full().child(
                    components::base_button(("tts-voice-choice", index))
                        .debug_selector(move || format!("tts-voice-choice-{index}"))
                        .ghost()
                        .selected(checked)
                        .flex_1()
                        .min_w_0()
                        .h_auto()
                        .py(px(10.0))
                        .tooltip(if checked { format!(text!(reader::unload), item.name) } else { item.name.clone() })
                        .child(
                            components::column(0.0)
                                .items_start()
                                .min_w_0()
                                .flex_1()
                                .child(item.name.clone())
                                .children(item.detail.clone().map(|detail| components::caption_line(detail, theme).when(checked, |line| line.text_color(theme.accent_text)))),
                        )
                        .children(checked.then(|| Icon::new(IconName::Check)))
                        .on_click(cx.listener(move |picker, _, _, cx| picker.select(&choice, cx))),
                )
            })
            .collect::<Vec<_>>();
        let message = if self.device {
            if self.snapshot.loading {
                Some(text!(reader::finding_installed_local_voices).to_owned())
            } else if let Some(error) = &self.snapshot.error {
                Some(error.clone())
            } else if choices.is_empty() {
                Some(text!(reader::no_matching_local_voices_are_installed_install_an_offline_voice_in_device).into())
            } else {
                None
            }
        } else if choices.is_empty() {
            Some(text!(reader::no_built_in_voice_supports_this_books_language_on_this_device).into())
        } else {
            None
        };
        div()
            .key_context("TtsVoicePicker")
            .track_focus(&self.focus)
            .tab_group()
            .w_full()
            .flex()
            .flex_col()
            .max_h(px(480.0))
            .text_color(theme.text)
            .on_key_down(cx.listener(|picker, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    (picker.close)(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(tabs)
            .child(div().id("tts-voice-list").flex_1().min_h_0().overflow_y_scroll().children(message.map(|text| components::caption_line(text, theme).p(px(12.0)))).children(rows))
            .child(components::base_button("tts-refresh-voices").ghost().label(text!(reader::refresh_local_voices)).on_click(cx.listener(|picker, _, _, cx| {
                let _ = picker.owner.update(cx, |reader, cx| {
                    reader.narration_mut().reload_voices(cx);
                    cx.notify();
                });
            })))
    }
}

pub(super) fn button<C: TtsReader>(state: &Narration, compact: bool, cx: &mut Context<C>) -> AnyElement {
    let snapshot = Snapshot::new(state);
    let label = if state.voice == NarrationVoice::Device {
        state.device_voices.iter().find(|voice| Some(&voice.id) == state.device_voice.as_ref()).map_or(text!(reader::device_voice), |voice| voice.name.as_str())
    } else if state.voice == NarrationVoice::George {
        text!(reader::george)
    } else {
        text!(reader::alba)
    };
    let trigger = if compact {
        components::outlined_icon_button("tts-voice", format!(text!(reader::narration_voice), label = label), Icon::new(IconName::User).size(px(20.0))).p_0().size(px(32.0))
    } else {
        components::player_dock_text_button("tts-voice", label.to_owned()).tooltip(format!(text!(reader::narration_voice), label = label))
    }
    .debug_selector(|| "tts-voice".into())
    .no_fill();
    let owner = cx.weak_entity();
    if compact {
        trigger
            .on_click(move |_, window, cx| {
                let owner = owner.clone();
                let snapshot = snapshot.clone();
                window.open_sheet_at(gpui_component::Placement::Bottom, cx, move |sheet, window, cx| {
                    let Some(owner) = owner.upgrade() else {
                        return sheet;
                    };
                    let picker = window.use_keyed_state(("tts-voice-sheet", owner.entity_id()), cx, |window, cx| Picker::new(&owner, snapshot.clone(), Rc::new(|window, cx| window.close_sheet(cx)), window, cx));
                    sheet.title(text!(reader::narration_voice_label)).size(px(420.0)).resizable(false).p_0().child(picker)
                });
            })
            .into_any_element()
    } else {
        Popover::new("tts-local-voices")
            .anchor(Anchor::BottomRight)
            .trigger(trigger)
            .content(move |_, window, cx| {
                let Some(owner) = owner.upgrade() else {
                    return div().into_any_element();
                };
                let popover = cx.entity().downgrade();
                let close: Close = Rc::new(move |window, cx| {
                    let _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                });
                let picker = window.use_keyed_state(("tts-voice-popup", owner.entity_id()), cx, |window, cx| Picker::new(&owner, snapshot.clone(), close.clone(), window, cx));
                picker.update(cx, |picker, cx| {
                    picker.close = close;
                    if picker.snapshot != snapshot {
                        picker.snapshot = snapshot.clone();
                        cx.notify();
                    }
                });
                div().w(px(360.0)).max_w_full().child(picker).into_any_element()
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ReaderPreferences, ReaderSettings};
    use gpui::{AppContext, Modifiers, TestAppContext};

    #[test]
    fn choices_match_declared_book_languages_and_show_all_when_unspecified() {
        let voices = vec![
            DeviceVoice { id: "uk".into(), name: "British English".into(), language: "en-GB".into() },
            DeviceVoice { id: "us".into(), name: "American English".into(), language: "en-US".into() },
            DeviceVoice { id: "sv".into(), name: "Swedish".into(), language: "sv-SE".into() },
        ];
        for (languages, expected) in [
            (vec!["en"], vec!["us", "uk"]),
            (vec!["en-US"], vec!["us", "uk"]),
            (vec!["swe"], vec!["sv"]),
            (vec!["und", "sv"], vec!["sv"]),
            (vec!["sv", "en"], vec!["us", "uk", "sv"]),
            (vec!["fr"], vec![]),
            (vec![], vec!["us", "uk", "sv"]),
            (vec![""], vec!["us", "uk", "sv"]),
            (vec!["und"], vec!["us", "uk", "sv"]),
        ] {
            let mut state = Narration::for_languages(&languages.iter().map(|language| (*language).into()).collect::<Vec<_>>());
            state.apply_voices(Ok(voices.clone()));
            let snapshot = Snapshot::new(&state);
            let ids = snapshot
                .device
                .iter()
                .map(|item| match &item.choice {
                    Choice::Device(id) => id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, expected, "book languages: {languages:?}");
            #[cfg(not(target_arch = "wasm32"))]
            assert_eq!(snapshot.bundled.is_empty(), !expected.contains(&"uk"), "book languages: {languages:?}");
        }
    }

    #[test]
    fn voice_order_is_grouped_by_language_then_name_and_independent_of_inventory_order() {
        let mut voices = vec![
            DeviceVoice { id: "sv".into(), name: "Alice".into(), language: "sv-SE".into() },
            DeviceVoice { id: "z".into(), name: "Zoe".into(), language: "en-GB".into() },
            DeviceVoice { id: "b".into(), name: "bob".into(), language: "en-US".into() },
            DeviceVoice { id: "a2".into(), name: "Alice".into(), language: "eng".into() },
            DeviceVoice { id: "a1".into(), name: "Alice".into(), language: "en-GB".into() },
        ];
        for _ in 0..2 {
            let mut state = Narration::for_languages(&[]);
            state.apply_voices(Ok(voices.clone()));
            let snapshot = Snapshot::new(&state);
            let ids = snapshot
                .device
                .iter()
                .map(|item| match &item.choice {
                    Choice::Device(id) => id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            assert_eq!(ids, ["a1", "a2", "b", "z", "sv"]);
            voices.reverse();
        }
    }

    #[test]
    fn selected_voice_can_be_unloaded_but_the_last_matching_voice_is_kept() {
        let mut state = Narration::for_languages(&[]).with_voice(NarrationVoice::Device).with_device_voice(Some("english".into()));
        state.apply_voices(Ok(vec![DeviceVoice { id: "english".into(), name: "English".into(), language: "en-GB".into() }, DeviceVoice { id: "swedish".into(), name: "Swedish".into(), language: "sv-SE".into() }]));
        state.current = Some(12);
        state.remember_voice("sv", NarrationVoice::Device, Some("swedish".into()));
        let snapshot = Snapshot::new(&state);
        let swedish = snapshot.device[1].clone();
        assert!(snapshot.toggle(&swedish, &mut state));
        assert_eq!(state.device_voice.as_deref(), Some("english"));
        assert_eq!(state.current, Some(12));
        assert!(!state.voice_preferences.contains_key("sv"));
        let snapshot = Snapshot::new(&state);
        assert!(!snapshot.toggle(&snapshot.device[0], &mut state));
        assert_eq!(state.device_voice.as_deref(), Some("english"));

        assert!(Snapshot::new(&state).toggle(&swedish, &mut state));
        let snapshot = Snapshot::new(&state);
        assert_eq!(state.device_voice.as_deref(), Some("swedish"));
        assert!(snapshot.toggle(&swedish, &mut state));
        assert_eq!(state.device_voice.as_deref(), Some("english"));
        assert_eq!(state.current, Some(12));
        assert!(!state.playing && state.service.is_none());
        assert!(!state.voice_preferences.contains_key("sv"));
    }

    #[test]
    fn a_saved_voice_for_another_book_language_does_not_allow_unloading_the_last_match() {
        let mut state = Narration::for_languages(&["en".into()]).with_voice(NarrationVoice::Device).with_device_voice(Some("english".into()));
        state.apply_voices(Ok(vec![DeviceVoice { id: "english".into(), name: "English".into(), language: "en-GB".into() }, DeviceVoice { id: "swedish".into(), name: "Swedish".into(), language: "sv-SE".into() }]));
        state.remember_voice("sv", NarrationVoice::Device, Some("swedish".into()));
        let snapshot = Snapshot::new(&state);
        assert_eq!(snapshot.device.len(), 1);
        assert!(!snapshot.toggle(&snapshot.device[0], &mut state));
        assert_eq!(state.device_voice.as_deref(), Some("english"));
        assert_eq!(state.voice_preferences.len(), 2);
    }

    struct Reader {
        state: Narration,
        compact: bool,
    }
    impl TtsReader for Reader {
        fn narration(&self) -> &Narration {
            &self.state
        }
        fn narration_mut(&mut self) -> &mut Narration {
            &mut self.state
        }
        fn chapter_key(&self, _: &App) -> usize {
            0
        }
        fn skip_tts_element(&mut self, _: bool, _: &mut Context<Self>) {}
        fn start_tts(&mut self, _: &mut Context<Self>) {
            panic!("Choosing a voice must not start playback");
        }
        fn toggle_tts_pause(&mut self, _: &mut Context<Self>) {}
        fn stop_tts(&mut self, _: &mut Context<Self>) {
            self.state.stop();
        }
        fn speech_event(&mut self, _: SpeechEvent, _: &mut Context<Self>) {}
    }
    impl Render for Reader {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(if self.compact { super::super::dock(&self.state, true, components::BrowserTheme::default(), cx) } else { button(&self.state, false, cx) })
                .children(gpui_component::Root::render_sheet_layer(window, cx).map(|sheet| gpui::deferred(sheet).with_priority(2)))
        }
    }
    fn choose_local_voice(compact: bool, cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            components::register_fonts(cx).unwrap();
            cx.set_global(ReaderSettings { preferences: ReaderPreferences::default(), save: Rc::new(|_, _, _| {}) });
        });
        let (root, visual) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|_| {
                let mut state = Narration::for_languages(&["sv".into(), "en".into()]).with_voice(NarrationVoice::Device).with_device_voice(Some("first".into()));
                state.current = Some(12);
                state.rate = Some(1.75);
                state.sleep = Some(SleepPlan::EndOfChapter(0));
                state.apply_voices(Ok(vec![
                    DeviceVoice { id: "first".into(), name: "An installed Swedish narrator with a long descriptive name".into(), language: "sv-SE".into() },
                    DeviceVoice { id: "second".into(), name: "Second local voice".into(), language: "sv-SE".into() },
                    DeviceVoice { id: "english".into(), name: "English voice".into(), language: "en-GB".into() },
                ]));
                Reader { state, compact }
            });
            gpui_component::Root::new(owner, window, cx).bordered(!compact)
        });
        visual.run_until_parked();
        if compact {
            for width in [320.0, 360.0, 390.0] {
                visual.simulate_resize(gpui::size(px(width), px(640.0)));
                visual.run_until_parked();
                let dock = visual.debug_bounds("tts-dock").unwrap();
                let play = visual.debug_bounds("tts-play").unwrap();
                assert!((f32::from(play.center().x - dock.center().x)).abs() < 1.0, "play must remain centred at {width}px");
                let mut previous_right = dock.left();
                for selector in ["tts-voice", "tts-speed", "tts-pauses", "tts-back", "tts-play", "tts-next", "tts-sleep", "tts-close"] {
                    let control = visual.debug_bounds(selector).unwrap();
                    assert!(control.left() >= previous_right && control.right() <= dock.right(), "{selector} must be visible without overlaps at {width}px");
                    previous_right = control.right();
                }
            }
        }
        let trigger = visual.debug_bounds("tts-voice").unwrap();
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        visual.executor().advance_clock(Duration::from_millis(200));
        visual.run_until_parked();
        assert!(visual.debug_bounds("tts-device-tab").is_some());
        assert!(visual.debug_bounds("tts-add-voices").is_none());
        let choice = visual.debug_bounds("tts-voice-choice-2").unwrap();
        visual.simulate_click(choice.center(), Modifiers::default());
        visual.run_until_parked();
        root.read_with(visual, |root, cx| {
            let owner = root.view().clone().downcast::<Reader>().ok().unwrap();
            let state = &owner.read(cx).state;
            assert_eq!(state.device_voice.as_deref(), Some("second"));
            assert_eq!(state.current, Some(12));
            assert!(!state.playing && state.service.is_none());
            assert_eq!(ReaderSettings::preferences(cx).narration_device_voice.as_deref(), Some("second"));
        });
        let english = visual.debug_bounds("tts-voice-choice-0").unwrap();
        visual.simulate_click(english.center(), Modifiers::default());
        visual.run_until_parked();
        root.read_with(visual, |root, cx| {
            let owner = root.view().clone().downcast::<Reader>().ok().unwrap();
            let state = &owner.read(cx).state;
            assert_eq!(state.device_voice.as_deref(), Some("second"));
            assert_eq!(state.current, Some(12));
            let preferences = &ReaderSettings::preferences(cx).narration_voices;
            assert_eq!(preferences["sv"].device_voice.as_deref(), Some("second"));
            assert_eq!(preferences["en"].device_voice.as_deref(), Some("english"));
            let snapshot = Snapshot::new(state);
            assert_eq!(snapshot.preferences.len(), 2);
        });
        visual.simulate_click(english.center(), Modifiers::default());
        visual.run_until_parked();
        root.read_with(visual, |_, cx| {
            let preferences = &ReaderSettings::preferences(cx).narration_voices;
            assert_eq!(preferences.len(), 1);
            assert_eq!(preferences["sv"].device_voice.as_deref(), Some("second"));
        });
        visual.simulate_click(choice.center(), Modifiers::default());
        visual.run_until_parked();
        root.read_with(visual, |_, cx| {
            assert_eq!(ReaderSettings::preferences(cx).narration_voices["sv"].device_voice.as_deref(), Some("second"));
        });
        let embedded = visual.debug_bounds("tts-bokheim-tab").unwrap();
        visual.simulate_click(embedded.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("tts-add-voices").is_none());
        let close = visual.debug_bounds("tts-voice-picker-close").unwrap();
        visual.simulate_click(close.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("tts-device-tab").is_none());
    }
    #[gpui::test]
    fn desktop_popup_selects_a_local_voice_without_borrowing_the_reader_twice(cx: &mut TestAppContext) {
        choose_local_voice(false, cx);
    }
    #[gpui::test]
    fn mobile_sheet_selects_a_local_voice_without_starting_playback(cx: &mut TestAppContext) {
        choose_local_voice(true, cx);
    }
}
