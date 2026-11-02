use super::*;
use dictionary::{ClientFuture, DictionaryDownloads, DictionaryFile, DictionaryInfo, DictionaryMatches};
use gpui::{Entity, TestAppContext, point, size};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tokio::sync::watch;

fn empty_popup(anchor: Bounds<Pixels>, window: &mut Window, cx: &mut Context<DictionaryPopup>) -> DictionaryPopup {
    let focus = cx.focus_handle();
    focus.focus(window, cx);
    DictionaryPopup {
        translation: None,
        backend: None,
        settings: None,
        query: "Haus".into(),
        result: Some(Ok(DictionaryLookup { dictionaries: Vec::new(), enabled_count: 0, errors: Vec::new() })),
        dictionary_index: 0,
        definitions_scroll: ScrollHandle::new(),
        wheel_delta: 0.0,
        wheel_debounce: None,
        wheel_scrolling_text: false,
        touchpad: components::TouchpadNavigation::default(),
        downloading: false,
        selection_only: false,
        anchor: Some(anchor),
        element_click: WordClick::default(),
        focus,
        wikipedia_request: None,
        wikipedia_popup: None,
        wikipedia: None,
        languages: Vec::new(),
        wikipedia_attempted: false,
        wikipedia_hits: Vec::new(),
        wikipedia_index: 0,
        wikipedia_cache: BTreeMap::new(),
        wikipedia_loading: false,
        _dismiss_task: Task::ready(()),
        _wikipedia_task: Task::ready(()),
        _task: Task::ready(()),
    }
}

fn dictionary_match() -> DictionaryLookup {
    DictionaryLookup { dictionaries: vec![DictionaryMatches { dictionary: "Offline".into(), definitions: vec![DictionaryDefinition { word: "Haus".into(), text: "A building.".into() }] }], enabled_count: 1, errors: Vec::new() }
}

fn multiple_dictionary_matches() -> DictionaryLookup {
    DictionaryLookup {
        enabled_count: 3,
        errors: Vec::new(),
        dictionaries: vec![
            DictionaryMatches { dictionary: "No match".into(), definitions: Vec::new() },
            DictionaryMatches {
                dictionary: "German–English".into(),
                definitions: vec![DictionaryDefinition { word: "Haus".into(), text: "1. A house.\n2. A household.\n".repeat(40) }, DictionaryDefinition { word: "Häuser".into(), text: "Houses; plural of Haus.".into() }],
            },
            DictionaryMatches { dictionary: "German".into(), definitions: vec![DictionaryDefinition { word: "Haus".into(), text: "Ein Gebäude.".into() }] },
        ],
    }
}

struct WikipediaFixture(Entity<DictionaryPopup>);

impl Render for WikipediaFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

struct SettingsClient {
    languages: Rc<RefCell<Vec<Vec<String>>>>,
    changes: watch::Sender<()>,
}

fn unexpected_request<T>() -> ClientFuture<'static, T> {
    Box::pin(async { Err("Unexpected dictionary request".into()) })
}

impl DictionaryClient for SettingsClient {
    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads> {
        self.languages.borrow_mut().push(languages);
        Box::pin(async { Ok(DictionaryDownloads { available: Vec::new(), recommended: Vec::new(), active: None, error: None }) })
    }
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>> {
        Box::pin(async { Ok(self.changes.subscribe()) })
    }
    fn import(&self, _: Vec<DictionaryFile>) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        unexpected_request()
    }
    fn download(&self, _: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        unexpected_request()
    }
    fn configure(&self, _: String, _: bool, _: i8) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        unexpected_request()
    }
    fn remove(&self, _: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        unexpected_request()
    }
    fn lookup(&self, _: String, _: Vec<String>) -> ClientFuture<'_, DictionaryLookup> {
        unexpected_request()
    }
}

#[gpui::test]
fn dictionary_settings_return_to_the_same_lookup(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let languages = Rc::new(RefCell::new(Vec::new()));
    let backend: Rc<dyn DictionaryClient> = Rc::new(SettingsClient { languages: languages.clone(), changes: watch::channel(()).0 });
    let (fixture, visual) = cx.add_window_view(|window, cx| {
        WikipediaFixture(cx.new(|cx| {
            let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
            popup.backend = Some(backend);
            popup.languages = vec!["de-DE".into()];
            popup.result = Some(Ok(multiple_dictionary_matches()));
            popup.dictionary_index = 1;
            popup
        }))
    });
    visual.run_until_parked();
    let panel = visual.debug_bounds("dictionary-popup").unwrap();
    let counter = visual.debug_bounds("lookup-counter").unwrap();
    assert!(f32::from(counter.center().x - panel.center().x).abs() < 1.0, "settings must leave the result counter centered");
    for close_with_escape in [true, false] {
        let button = visual.debug_bounds("dictionary-settings").unwrap();
        visual.simulate_click(button.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("reader-dictionary-settings").is_some());
        assert!(visual.debug_bounds("import-stardict").is_some());
        assert!(visual.debug_bounds("download-dictionary").is_some());
        assert!(visual.debug_bounds("dictionary-popup").is_none());
        assert_eq!(languages.borrow().last().unwrap().first().map(String::as_str), Some("de-DE"));
        if close_with_escape {
            visual.simulate_keystrokes("escape");
        } else {
            let close = visual.debug_bounds("dictionary-settings-close").unwrap();
            visual.simulate_click(close.center(), Default::default());
        }
        visual.run_until_parked();
        assert!(visual.debug_bounds("dictionary-popup").is_some());
        fixture.read_with(visual, |fixture, cx| {
            let popup = fixture.0.read(cx);
            assert!(popup.settings.is_none());
            assert_eq!(popup.query, "Haus");
            assert_eq!(popup.languages, ["de-DE"]);
            assert_eq!(popup.dictionary_index, 1);
        });
    }
}

#[gpui::test]
fn successful_wikipedia_is_restored_without_retry_when_dictionary_availability_changes(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for reason in ["download", "match", "error", "pending"] {
        let attempts = Rc::new(Cell::new(0));
        let (fixture, visual) = cx.add_window_view(|window, cx| {
            WikipediaFixture(cx.new(|cx| {
                let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
                let attempts = attempts.clone();
                popup.wikipedia_request = Some(Rc::new(move |_, _| {
                    attempts.set(attempts.get() + 1);
                    Box::pin(std::future::ready(Some(WikipediaLookup::Article(WikipediaArticle { title: "Haus".into(), extract: "A building.".into(), url: "https://de.wikipedia.org/wiki/Haus".into(), language: "de".into() }))))
                }));
                popup.refresh_wikipedia(cx);
                popup
            }))
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("wikipedia-summary").is_some());
        fixture.update(visual, |fixture, cx| {
            fixture.0.update(cx, |popup, cx| {
                match reason {
                    "download" => popup.downloading = true,
                    "match" => popup.result = Some(Ok(dictionary_match())),
                    "error" => popup.result = Some(Err("Local lookup failed".into())),
                    "pending" => popup.result = None,
                    _ => unreachable!(),
                }
                popup.refresh_wikipedia(cx);
                assert!(popup.wikipedia.is_some(), "temporary unavailability must retain the successful article");
                assert!(popup.visible_wikipedia().is_none());
                cx.notify();
            })
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("wikipedia-summary").is_none(), "Wikipedia must be hidden during {reason}");
        assert!(visual.debug_bounds("wikipedia-title").is_none());
        fixture.update(visual, |fixture, cx| {
            fixture.0.update(cx, |popup, cx| {
                popup.downloading = false;
                popup.result = Some(Ok(DictionaryLookup { dictionaries: Vec::new(), enabled_count: 0, errors: Vec::new() }));
                popup.refresh_wikipedia(cx);
                assert_eq!(popup.visible_wikipedia().unwrap().title, "Haus");
                cx.notify();
            })
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("wikipedia-summary").is_some());
        assert_eq!(attempts.get(), 1, "restoring a cached article must not issue another request: {reason}");
    }
}

#[gpui::test]
fn dictionary_entries_page_across_dictionaries_without_network_and_reset_scroll(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let attempts = Rc::new(Cell::new(0));
    let (fixture, visual) = cx.add_window_view(|window, cx| {
        WikipediaFixture(cx.new(|cx| {
            let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
            popup.result = Some(Ok(multiple_dictionary_matches()));
            let attempts = attempts.clone();
            popup.wikipedia_request = Some(Rc::new(move |_, _| {
                attempts.set(attempts.get() + 1);
                Box::pin(std::future::ready(None))
            }));
            popup.refresh_wikipedia(cx);
            popup
        }))
    });
    visual.run_until_parked();
    fixture.update(visual, |fixture, cx| {
        fixture.0.update(cx, |popup, cx| {
            popup.definitions_scroll.set_offset(point(px(0.0), px(-80.0)));
            cx.notify();
        })
    });
    visual.run_until_parked();
    fixture.read_with(visual, |fixture, cx| assert!(fixture.0.read(cx).definitions_scroll.offset().y < px(0.0)));
    for (button, index, dictionary, word) in [
        ("lookup-next", 1, "German–English", "Häuser"),
        ("lookup-next", 2, "German", "Haus"),
        ("lookup-next", 0, "German–English", "Haus"),
        ("lookup-previous", 2, "German", "Haus"),
        ("lookup-previous", 1, "German–English", "Häuser"),
        ("lookup-previous", 0, "German–English", "Haus"),
    ] {
        let bounds = visual.debug_bounds(button).unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        visual.run_until_parked();
        fixture.read_with(visual, |fixture, cx| {
            let popup = fixture.0.read(cx);
            assert_eq!(popup.dictionary_entries().count(), 3, "empty dictionaries are excluded; senses remain together");
            assert_eq!(popup.dictionary_index, index);
            let (source, entry) = popup.dictionary_entries().nth(index).unwrap();
            assert_eq!(source, dictionary);
            assert_eq!(entry.word, word);
            assert_eq!(popup.definitions_scroll.offset(), Point::default());
        });
    }
    let panel = visual.debug_bounds("dictionary-popup").unwrap();
    let counter = visual.debug_bounds("lookup-counter").unwrap();
    assert!(f32::from(counter.center().x - panel.center().x).abs() < 1.0);
    fixture.update(visual, |fixture, cx| {
        fixture.0.update(cx, |popup, cx| {
            popup.dictionary_index = 2;
            popup.result = Some(Ok(dictionary_match()));
            cx.notify();
        })
    });
    visual.run_until_parked();
    fixture.read_with(visual, |fixture, cx| assert_eq!(fixture.0.read(cx).dictionary_index, 0));
    assert!(visual.debug_bounds("lookup-counter").is_none(), "a single entry needs no pager");
    assert_eq!(attempts.get(), 0, "paging through local matches must not query Wikipedia");
}

#[gpui::test]
fn wikipedia_runs_only_after_a_clean_dictionary_miss_and_failure_is_not_retried(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for scenario in ["miss", "match", "pending", "error", "dictionary-error", "downloading", "selection"] {
        let attempts = Rc::new(Cell::new(0));
        let (fixture, visual) = cx.add_window_view(|window, cx| {
            WikipediaFixture(cx.new(|cx| {
                let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
                let attempts = attempts.clone();
                popup.wikipedia_request = Some(Rc::new(move |_, _| {
                    attempts.set(attempts.get() + 1);
                    Box::pin(std::future::ready(None))
                }));
                match scenario {
                    "match" => popup.result = Some(Ok(dictionary_match())),
                    "pending" => popup.result = None,
                    "error" => popup.result = Some(Err("Local lookup failed".into())),
                    "dictionary-error" => popup.result.as_mut().unwrap().as_mut().unwrap().errors.push("Unreadable dictionary".into()),
                    "downloading" => popup.downloading = true,
                    "selection" => popup.selection_only = true,
                    _ => {}
                }
                popup.refresh_wikipedia(cx);
                popup
            }))
        });
        visual.run_until_parked();
        for _ in 0..3 {
            fixture.update(visual, |fixture, cx| fixture.0.update(cx, |popup, cx| popup.refresh_wikipedia(cx)));
            visual.run_until_parked();
        }
        assert_eq!(attempts.get(), usize::from(scenario == "miss"), "{scenario}");
        fixture.read_with(visual, |fixture, cx| {
            let popup = fixture.0.read(cx);
            assert!(popup.wikipedia.is_none());
            assert!(!popup.wikipedia_loading);
        });
        assert!(visual.debug_bounds("wikipedia-summary").is_none());
    }
}

#[gpui::test]
fn wikipedia_pager_caches_successes_and_failed_hits_without_retries(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let attempts = Rc::new(RefCell::new(Vec::new()));
    let (fixture, visual) = cx.add_window_view(|window, cx| {
        WikipediaFixture(cx.new(|cx| {
            let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
            popup.wikipedia_attempted = true;
            popup.wikipedia_hits = ["Mercury (planet)", "Mercury (element)", "Mercury (mythology)"]
                .into_iter()
                .map(|title| WikipediaChoice { title: title.into(), description: format!("About {title}"), language: "en".into(), url: "https://en.wikipedia.org/wiki/Mercury".into() })
                .collect();
            let attempts = attempts.clone();
            popup.wikipedia_request = Some(Rc::new(move |query, _| {
                attempts.borrow_mut().push(query.clone());
                let result =
                    (query != "Mercury (element)").then(|| WikipediaLookup::Article(WikipediaArticle { title: query, extract: "An article summary.".into(), url: "https://en.wikipedia.org/wiki/Mercury".into(), language: "en".into() }));
                Box::pin(std::future::ready(result))
            }));
            popup.wikipedia_hit(0, cx);
            popup
        }))
    });
    visual.run_until_parked();
    for (button, index, requests) in [("lookup-next", 1, 2), ("lookup-previous", 0, 2), ("lookup-next", 1, 2), ("lookup-next", 2, 3), ("lookup-next", 0, 3), ("lookup-previous", 2, 3), ("lookup-next", 0, 3)] {
        let bounds = visual.debug_bounds(button).unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        visual.run_until_parked();
        fixture.read_with(visual, |fixture, cx| {
            let popup = fixture.0.read(cx);
            assert_eq!(popup.wikipedia_index, index);
            assert!(!popup.wikipedia_loading);
            let Some(article) = &popup.wikipedia else { panic!("expected a visible article") };
            assert_eq!(article.title, popup.wikipedia_hits[index].title);
            if index == 1 {
                assert_eq!(article.extract, "About Mercury (element)");
            }
        });
        assert_eq!(attempts.borrow().len(), requests);
    }
    let panel = visual.debug_bounds("dictionary-popup").unwrap();
    let counter = visual.debug_bounds("lookup-counter").unwrap();
    assert!(f32::from(counter.center().x - panel.center().x).abs() < 1.0, "the hit counter must be centered");
    fixture.update(visual, |fixture, cx| {
        fixture.0.update(cx, |popup, cx| {
            popup.wikipedia_hit(1, cx);
            popup.downloading = true;
            popup.refresh_wikipedia(cx);
            cx.notify();
        })
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("lookup-counter").is_none());
    assert!(visual.debug_bounds("wikipedia-summary").is_none());
    fixture.update(visual, |fixture, cx| {
        fixture.0.update(cx, |popup, cx| {
            popup.downloading = false;
            popup.refresh_wikipedia(cx);
            assert_eq!(popup.wikipedia_hits.len(), 3);
            assert_eq!(popup.wikipedia_index, 1);
            assert_eq!(popup.visible_wikipedia().unwrap().extract, "About Mercury (element)");
            popup.wikipedia_hit(0, cx);
            popup.wikipedia_hit(1, cx);
            cx.notify();
        })
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("lookup-counter").is_some());
    assert_eq!(attempts.borrow().len(), 3, "restoring the pager must preserve successful and failed attempt markers");
}

#[gpui::test]
fn dictionary_match_cancels_an_inflight_wikipedia_fallback(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (fixture, visual) = cx.add_window_view(|window, cx| {
        WikipediaFixture(cx.new(|cx| {
            let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
            popup.wikipedia_request = Some(Rc::new(|_, _| Box::pin(std::future::pending())));
            popup.refresh_wikipedia(cx);
            popup
        }))
    });
    visual.run_until_parked();
    fixture.update(visual, |fixture, cx| {
        fixture.0.update(cx, |popup, cx| {
            assert!(popup.wikipedia_loading);
            popup.result = Some(Ok(dictionary_match()));
            popup.refresh_wikipedia(cx);
            assert!(!popup.wikipedia_loading);
            assert!(popup.wikipedia.is_none());
        })
    });
}

#[test]
fn word_selection_requires_an_unmodified_double_or_fourth_click_without_a_drag() {
    let mut click = WordClick::default();
    let position = point(px(100.0), px(100.0));
    for (count, button, distance, modified, expected) in [
        (1, MouseButton::Left, 0.0, false, false),
        (2, MouseButton::Left, 0.0, false, true),
        (2, MouseButton::Left, 3.0, false, true),
        (2, MouseButton::Left, 4.0, false, false),
        (2, MouseButton::Left, 5.0, false, false),
        (3, MouseButton::Left, 0.0, false, false),
        (4, MouseButton::Left, 0.0, false, true),
        (4, MouseButton::Left, 20.0, false, false),
        (2, MouseButton::Right, 0.0, false, false),
        (2, MouseButton::Left, 20.0, false, false),
        (2, MouseButton::Left, 0.0, true, false),
    ] {
        let modifiers = gpui::Modifiers { shift: modified, ..Default::default() };
        click.begin(&MouseDownEvent { position, button, click_count: count, modifiers, ..Default::default() });
        let release = MouseUpEvent { position: position + point(px(distance), px(0.0)), button, click_count: count, modifiers };
        assert_eq!(click.finish(&release), expected);
        assert!(!click.finish(&release), "a pointer release must be consumed only once");
    }
}

#[test]
fn word_click_yields_to_short_word_selection_drags() {
    use html::geometry::WordHit;
    use html::selection::{SelectionMode, SelectionState};

    let position = point(px(100.0), px(100.0));
    for (dx, dy, dragged) in [(2.0, 2.0, false), (4.0, 0.0, true), (5.0, 0.0, true), (3.0, 3.0, true)] {
        let mut click = WordClick::default();
        click.begin(&MouseDownEvent { position, button: MouseButton::Left, click_count: 2, ..Default::default() });
        let end = position + point(px(dx), px(dy));
        click.moved(end);
        let mut selection = SelectionState::default();
        selection.begin_at_word(WordHit { caret: 1, word: 0..1, position: (100.0, 100.0).into() }, SelectionMode::Plain);
        let hit = WordHit { caret: 1, word: 1..2, position: (100.0 + f64::from(dx), 100.0 + f64::from(dy)).into() };
        assert_eq!(selection.update_word_interaction(Some(hit), SelectionMode::Plain), dragged);
        let up = MouseUpEvent { position: end, button: MouseButton::Left, click_count: 2, ..Default::default() };
        assert_eq!(click.finish(&up), !dragged, "motion ({dx}, {dy}) must have one interpretation");
    }
}

#[test]
fn word_lookup_rejects_a_drag_that_returns_to_the_start() {
    let mut click = WordClick::default();
    let position = point(px(100.0), px(100.0));
    let down = MouseDownEvent { position, button: MouseButton::Left, click_count: 2, ..Default::default() };
    let up = MouseUpEvent { position, button: MouseButton::Left, click_count: 2, ..Default::default() };
    for delta in [point(px(20.0), px(0.0)), point(px(0.0), px(20.0)), point(px(4.0), px(0.0)), point(px(3.0), px(3.0))] {
        click.begin(&down);
        click.moved(position + delta);
        click.moved(position);
        assert!(!click.finish(&up));
    }
    click.begin(&down);
    click.moved(position + point(px(2.0), px(2.0)));
    assert!(click.finish(&up), "ordinary pointer jitter remains a double-click");
}

struct LookupFixture {
    popup: Option<Entity<DictionaryPopup>>,
    closed: usize,
    book_clicks: usize,
    book_scrolls: usize,
    expanded: usize,
}

impl Render for LookupFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_scroll_wheel(cx.listener(|this, _, _, _| this.book_scrolls += 1))
            .child(div().id("lookup-book").size_full().on_click(cx.listener(|this, _, _, _| this.book_clicks += 1)).child(components::outlined_text_button("other-control", "Other control")))
            .children(self.popup.clone())
    }
}

#[gpui::test]
fn lookup_can_expand_and_dismiss_without_clicking_the_book(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for action in ["outside", "escape", "expand-four", "expand-two", "drag-four", "drag-two", "drag-return", "drag-popup-return", "move-anchor"] {
        let anchor = if action == "outside" { point(px(735.0), px(550.0)) } else { point(px(200.0), px(100.0)) };
        let (fixture, visual) = cx.add_window_view(|window, cx| {
            let popup = cx.new(|cx| empty_popup(Bounds::new(anchor, size(px(45.0), px(22.0))), window, cx));
            cx.subscribe_in(&popup, window, |this: &mut LookupFixture, _, _: &Closed, _, cx| {
                this.closed += 1;
                this.popup = None;
                cx.notify();
            })
            .detach();
            cx.subscribe_in(&popup, window, |this: &mut LookupFixture, popup, _: &SelectElement, _, cx| {
                this.expanded += 1;
                popup.update(cx, |popup, cx| popup.show_selection("A complete paragraph with inline text.".into(), Some(Bounds::new(point(px(100.0), px(50.0)), size(px(600.0), px(500.0)))), cx));
            })
            .detach();
            LookupFixture { popup: Some(popup), closed: 0, book_clicks: 0, book_scrolls: 0, expanded: 0 }
        });
        visual.simulate_resize(size(px(800.0), px(600.0)));
        visual.run_until_parked();
        let bounds = visual.debug_bounds("dictionary-popup").unwrap();
        assert!(bounds.top() >= anchor.y + px(30.0) || bounds.bottom() <= anchor.y - px(8.0), "the popup must leave the selected word visible: {action}, popup={bounds:?}, word={anchor:?}");
        assert!(bounds.right() <= px(800.0) && bounds.bottom() <= px(600.0));
        assert!(visual.debug_bounds("dictionary-definitions").unwrap().size.height > px(20.0), "the result message must remain visible");
        for position in [bounds.center(), point(px(700.0), px(50.0))] {
            visual.simulate_event(gpui::ScrollWheelEvent { position, delta: gpui::ScrollDelta::Pages(point(0.0, -1.0)), ..Default::default() });
        }
        let expands = action.starts_with("expand");
        let drags = action.starts_with("drag");
        if expands || drags {
            for count in if action.ends_with("four") { [3, 4] } else { [1, 2] } {
                let position = anchor + point(px(20.0), px(10.0));
                visual.simulate_event(MouseDownEvent { button: MouseButton::Left, position, click_count: count, ..Default::default() });
                let position = if drags && matches!(count, 2 | 4) {
                    let moved = if action == "drag-popup-return" { bounds.center() } else { position + point(px(20.0), px(0.0)) };
                    visual.simulate_event(gpui::MouseMoveEvent { position: moved, pressed_button: Some(MouseButton::Left), ..Default::default() });
                    if action.ends_with("return") {
                        visual.simulate_event(gpui::MouseMoveEvent { position, pressed_button: Some(MouseButton::Left), ..Default::default() });
                        position
                    } else {
                        moved
                    }
                } else {
                    position
                };
                visual.simulate_event(MouseUpEvent { button: MouseButton::Left, position, click_count: count, ..Default::default() });
                visual.run_until_parked();
            }
            fixture.read_with(visual, |fixture, cx| {
                let popup = fixture.popup.as_ref().expect("expanding must retain the popup").read(cx);
                assert_eq!(popup.selection_only, expands, "dragging must not expand the selection: {action}");
                assert_eq!(popup.query, if expands { "A complete paragraph with inline text." } else { "Haus" });
            });
        }
        if action == "move-anchor" {
            let paragraph = Bounds::new(point(px(100.0), px(220.0)), size(px(600.0), px(160.0)));
            fixture.update(visual, |fixture, cx| fixture.popup.as_ref().unwrap().update(cx, |popup, cx| popup.set_anchor(Some(paragraph), cx)));
            visual.run_until_parked();
            let bounds = visual.debug_bounds("dictionary-popup").unwrap();
            assert!(bounds.bottom() <= paragraph.top() - px(8.0) || bounds.top() >= paragraph.bottom() + px(8.0), "updated geometry must keep the popup clear of the paragraph");
        }
        assert!(visual.debug_bounds("dictionary-highlight").is_none());
        if action != "outside" {
            visual.simulate_keystrokes("escape");
        } else {
            visual.simulate_click(point(px(700.0), px(50.0)), Default::default());
        }
        visual.run_until_parked();
        fixture.read_with(visual, |fixture, _| {
            assert_eq!(fixture.closed, 1);
            assert_eq!(fixture.expanded, usize::from(expands));
            assert_eq!(fixture.book_clicks, 0, "popup input must never activate the book beneath it");
            assert_eq!(fixture.book_scrolls, 0, "popup scrolling must never turn a page");
            assert!(fixture.popup.is_none());
        });
    }
}

struct TrackingFixture {
    popup: Entity<DictionaryPopup>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    reads: Rc<Cell<usize>>,
}

impl Render for TrackingFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let measured = self.bounds.clone();
        let bounds = self.bounds.clone();
        let reads = self.reads.clone();
        div()
            .relative()
            .size_full()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, _, _| {
                        measured.set(Some(Bounds::new(point(bounds.size.width * 0.25, bounds.size.height * 0.2), size(px(45.0), px(22.0)))));
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(DictionaryPopup::track_anchor(self.popup.downgrade(), move |_| {
                reads.set(reads.get() + 1);
                bounds.get()
            }))
            .child(self.popup.clone())
    }
}

#[gpui::test]
fn lookup_tracks_current_document_geometry_after_resize(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (fixture, visual) =
        cx.add_window_view(|window, cx| TrackingFixture { popup: cx.new(|cx| empty_popup(Bounds::new(point(px(0.0), px(0.0)), size(px(45.0), px(22.0))), window, cx)), bounds: Rc::new(Cell::new(None)), reads: Rc::new(Cell::new(0)) });
    for (width, height) in [(800.0, 600.0), (420.0, 540.0), (900.0, 700.0)] {
        visual.simulate_resize(size(px(width), px(height)));
        visual.run_until_parked();
        let expected = Bounds::new(point(px(width * 0.25), px(height * 0.2)), size(px(45.0), px(22.0)));
        fixture.read_with(visual, |fixture, cx| {
            assert_eq!(fixture.popup.read(cx).anchor, Some(expected));
            assert!(fixture.reads.get() < 20, "unchanged geometry must settle without a repaint loop");
        });
        let popup = visual.debug_bounds("dictionary-popup").unwrap();
        assert!(popup.bottom() <= expected.top() - px(8.0) || popup.top() >= expected.bottom() + px(8.0));
        assert!(popup.left() >= px(0.0) && popup.right() <= px(width) && popup.bottom() <= px(height));
    }
}

#[gpui::test]
fn lookup_keyboard_focus_stays_inside_and_escape_closes_from_a_button(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for kind in ["empty", "wikipedia", "wikipedia-multiple", "dictionary"] {
        let has_article = kind.starts_with("wikipedia");
        let has_pager = kind == "dictionary";
        let has_alternatives = has_pager || kind == "wikipedia-multiple";
        let (root, visual) = cx.add_window_view(|window, cx| {
            let fixture = cx.new(|cx| {
                let popup = cx.new(|cx| {
                    let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
                    if has_pager {
                        popup.result = Some(Ok(multiple_dictionary_matches()));
                    }
                    if has_article {
                        popup.wikipedia = Some(WikipediaArticle { title: "Haus".into(), extract: "A building.".into(), url: "https://de.wikipedia.org/wiki/Haus".into(), language: "de".into() });
                        if has_alternatives {
                            for title in ["Haus", "Häuser", "Haushalt"] {
                                let mut article = popup.wikipedia.clone().unwrap();
                                article.title = title.into();
                                popup.wikipedia_hits.push(WikipediaChoice { title: article.title.clone(), description: article.extract.clone(), language: article.language.clone(), url: article.url.clone() });
                                popup.wikipedia_cache.insert(title.into(), Some(article));
                            }
                        }
                    }
                    popup
                });
                cx.subscribe(&popup, |fixture: &mut LookupFixture, _, _: &Closed, cx| {
                    fixture.closed += 1;
                    fixture.popup = None;
                    cx.notify();
                })
                .detach();
                LookupFixture { popup: Some(popup), closed: 0, book_clicks: 0, book_scrolls: 0, expanded: 0 }
            });
            gpui_component::Root::new(fixture, window, cx)
        });
        visual.run_until_parked();
        let fixture = visual.update(|_, cx| root.read(cx).view().clone().downcast::<LookupFixture>().ok().unwrap());
        let focus = fixture.read_with(visual, |fixture, cx| fixture.popup.as_ref().unwrap().read(cx).focus.clone());
        for key in ["tab", "tab", "tab", "tab", "tab", "shift-tab", "shift-tab", "shift-tab", "shift-tab", "shift-tab"] {
            visual.simulate_keystrokes(key);
            visual.update(|window, cx| {
                assert!(focus.contains_focused(window, cx), "{key} must not focus controls behind the popup");
                if has_article || has_pager {
                    assert_ne!(window.focused(cx), Some(focus.clone()), "Tab must reach an action button");
                }
            });
        }
        if has_alternatives {
            let popup = fixture.read_with(visual, |fixture, _| fixture.popup.clone().unwrap());
            visual.update(|window, cx| focus.focus(window, cx));
            for (key, expected) in [("left", 2), ("right", 0), ("right", 1), ("shift-right", 1), ("ctrl-right", 1), ("alt-right", 1), ("right", 2), ("right", 0), ("left", 2), ("left", 1), ("left", 0)] {
                visual.simulate_keystrokes(key);
                popup.read_with(visual, |popup, _| assert_eq!(if has_pager { popup.dictionary_index } else { popup.wikipedia_index }, expected, "{kind}: {key}"));
                visual.update(|window, cx| assert!(focus.contains_focused(window, cx), "arrow navigation must retain popup focus"));
            }
            visual.simulate_keystrokes("tab");
            if has_article {
                visual.simulate_keystrokes("tab");
                visual.simulate_keystrokes("tab");
            }
            visual.update(|window, cx| assert_ne!(window.focused(cx), Some(focus.clone()), "test arrow navigation from a result control too"));
            visual.simulate_keystrokes("right");
            popup.read_with(visual, |popup, _| assert_eq!(if has_pager { popup.dictionary_index } else { popup.wikipedia_index }, 1));
            visual.simulate_keystrokes("left");
            visual.simulate_keystrokes("tab");
        }
        if has_pager {
            for _ in 0..3 {
                visual.simulate_keystrokes("enter");
                visual.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), key_location: gpui::KeyLocation::Standard });
                visual.run_until_parked();
            }
            fixture.read_with(visual, |fixture, cx| assert_eq!(fixture.popup.as_ref().unwrap().read(cx).dictionary_index, 0, "Enter on Next must wrap around"));
            visual.update(|window, cx| assert!(focus.contains_focused(window, cx), "keyboard paging must retain popup focus"));
        }
        if has_article {
            if !has_alternatives {
                let article_focus = visual.update(|window, cx| window.focused(cx));
                visual.simulate_keystrokes("tab");
                visual.update(|window, cx| assert_eq!(window.focused(cx), article_focus, "Tab skips the mouse-only source and license trigger"));
            }
            assert!(visual.debug_bounds("wikipedia-attribution").is_none());
            let info = visual.debug_bounds("wikipedia-info-button").unwrap();
            visual.simulate_click(info.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds("wikipedia-attribution").is_some());
            visual.simulate_keystrokes("tab");
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
            assert!(visual.debug_bounds("wikipedia-attribution").is_none(), "Escape dismisses attribution first");
            fixture.read_with(visual, |fixture, _| assert!(fixture.popup.is_some(), "reading attribution must retain the article"));
        }
        visual.simulate_keystrokes("escape");
        fixture.read_with(visual, |fixture, _| {
            assert_eq!(fixture.closed, 1);
            assert!(fixture.popup.is_none());
            assert_eq!(fixture.book_clicks, 0);
        });
    }
}

#[gpui::test]
fn lookup_restores_focus_when_result_controls_disappear(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for change in ["selection", "download", "match", "error", "pending", "pager"] {
        let (root, visual) = cx.add_window_view(|window, cx| {
            let fixture = cx.new(|cx| {
                let popup = cx.new(|cx| {
                    let mut popup = empty_popup(Bounds::new(point(px(200.0), px(100.0)), size(px(45.0), px(22.0))), window, cx);
                    if change == "pager" {
                        popup.result = Some(Ok(multiple_dictionary_matches()));
                    } else {
                        popup.wikipedia = Some(WikipediaArticle { title: "Haus".into(), extract: "A building.".into(), url: "https://de.wikipedia.org/wiki/Haus".into(), language: "de".into() });
                    }
                    popup.wikipedia_attempted = true;
                    popup
                });
                cx.subscribe(&popup, |fixture: &mut LookupFixture, _, _: &Closed, cx| {
                    fixture.closed += 1;
                    fixture.popup = None;
                    cx.notify();
                })
                .detach();
                LookupFixture { popup: Some(popup), closed: 0, book_clicks: 0, book_scrolls: 0, expanded: 0 }
            });
            gpui_component::Root::new(fixture, window, cx)
        });
        visual.run_until_parked();
        let fixture = visual.update(|_, cx| root.read(cx).view().clone().downcast::<LookupFixture>().ok().unwrap());
        let popup = fixture.read_with(visual, |fixture, _| fixture.popup.clone().unwrap());
        let focus = popup.read_with(visual, |popup, _| popup.focus.clone());
        visual.simulate_keystrokes("tab");
        visual.update(|window, cx| {
            assert!(focus.contains_focused(window, cx));
            assert_ne!(window.focused(cx), Some(focus.clone()), "start with a result control focused");
        });
        popup.update(visual, |popup, cx| {
            match change {
                "selection" => popup.show_selection("An entire passage.".into(), popup.anchor, cx),
                "download" => popup.downloading = true,
                "match" | "pager" => popup.result = Some(Ok(dictionary_match())),
                "error" => popup.result = Some(Err("Local lookup failed".into())),
                "pending" => popup.result = None,
                _ => unreachable!(),
            }
            popup.refresh_wikipedia(cx);
            cx.notify();
        });
        visual.run_until_parked();
        visual.update(|window, cx| assert_eq!(window.focused(cx), Some(focus.clone()), "{change} must restore focus to the popup"));
        visual.simulate_keystrokes("tab");
        visual.update(|window, cx| assert!(focus.contains_focused(window, cx), "{change} must retain the focus trap"));
        visual.simulate_keystrokes("escape");
        fixture.read_with(visual, |fixture, _| {
            assert_eq!(fixture.closed, 1, "{change} must retain Escape dismissal");
            assert!(fixture.popup.is_none());
        });
    }
}
