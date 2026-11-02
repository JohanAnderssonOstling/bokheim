use super::*;
use crate::navigation::LibrarySession;
use book_model::AnnotationAnchor;
use gpui::{TestAppContext, size};
use gpui_component::Root;
use std::{cell::Cell, rc::Rc};
use sync_common::ROOT_DIR_ID;

struct Fixture;
impl Render for Fixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(Root::render_dialog_layer(window, cx))
    }
}

#[gpui::test]
async fn notebook_editor_confirms_saves_keeps_failed_drafts_and_gates_navigation(cx: &mut TestAppContext) {
    let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_directory, session, original) = LibrarySession::test_context(cx).await;
    let entry = original.app().create_library_with_asset_storage("Editor fixture".into(), false).await.unwrap();
    let opened = Rc::new(Cell::new(0));
    let recorded = opened.clone();
    let library = cx.update(|cx| {
        let app = Rc::new(original.app().clone());
        let backend = Rc::new(app.library(*entry.library_id()));
        let updates = crate::library::LibraryUpdateBridge::create(app.clone(), *entry.library_id(), cx);
        LibraryContext::new(app, backend, Rc::new(move |_, _, _, _, _| recorded.set(recorded.get() + 1)), updates, session.downgrade(), cx.new(|_| crate::NowPlaying::default()))
    });
    let backend = library.backend().clone();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/library-backend/crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    assert!(backend.import_paths(ROOT_DIR_ID, vec![fixture], false).await.unwrap().is_empty());
    let book = backend
        .browse_section_books(
            library_model::LibraryBrowseQuery {
                location: library_model::BrowseLocation::Books,
                search: String::new(),
                file_types: Vec::new(),
                chip_sort: Default::default(),
                book_sort: Default::default(),
                languages: Vec::new(),
                hide_finished: false,
            },
            0,
            1,
        )
        .await
        .unwrap()
        .books
        .remove(0);
    let id = book.library_file().unwrap();
    let annotation = Annotation {
        id: "editor-save-fixture".into(),
        content_hash: id,
        anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"),
        exact_text: "The complete highlighted passage.".into(),
        style: AnnotationStyle::Highlight,
        color: 0xe0bb4b,
        note: "Original note".into(),
        created_at: 1,
        modified_at: 1,
        toc_ordinal: None,
        progress: None,
        deleted: false,
    };
    backend.upsert_annotation(annotation.clone()).await.unwrap();
    let mut concurrent_color = annotation.clone();
    concurrent_color.color = 0x6e9dcb;
    backend.upsert_annotation(concurrent_color).await.unwrap();
    let (_root, visual) = cx.add_window_view(|window, cx| {
        let fixture = cx.new(|_| Fixture);
        Root::new(fixture, window, cx)
    });
    visual.simulate_resize(size(px(800.0), px(600.0)));
    let editor = visual.update(|window, cx| show_editor(library.clone(), book.title.clone(), annotation, window, cx));
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notebook-full-annotation").is_some());
    assert!(visual.debug_bounds("reader-annotation-style-row").is_some());
    assert!(visual.debug_bounds("reader-annotation-color-row").is_some());
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.input.update(cx, |input, cx| input.set_value("First edit", window, cx));
            editor.save_annotation_changes(Some(AnnotationStyle::Underline), None, cx);
            editor.input.update(cx, |input, cx| input.set_value("Final edit saved on dismissal", window, cx));
        })
    });
    visual.simulate_keystrokes("escape");
    visual.condition(&editor, |editor, _| matches!(editor.state, EditorState::Closed)).await;
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notebook-annotation-editor").is_none());
    assert!(editor.read_with(visual, |editor, _| matches!(editor.state, EditorState::Closed)));
    assert_eq!(editor.read_with(visual, |editor, _| editor.draft.note.clone()), "Final edit saved on dismissal");
    let saved = backend.annotations(id).await.unwrap().remove(0);
    assert_eq!(saved.note, "Final edit saved on dismissal");
    assert_eq!(saved.style, AnnotationStyle::Underline);
    assert_eq!(saved.color, 0xe0bb4b, "the latest whole-annotation save replaces an earlier color change");
    assert_eq!(editor.read_with(visual, |editor, _| editor.draft.color), saved.color);

    // Use the same last-save-wins policy as the reader, including when the
    // same note changed elsewhere while this editor held its earlier snapshot.
    let mut concurrent = saved.clone();
    concurrent.note = "Changed elsewhere".into();
    backend.upsert_annotation(concurrent).await.unwrap();
    let latest = visual.update(|window, cx| show_editor(library.clone(), book.title.clone(), saved.clone(), window, cx));
    visual.update(|window, cx| {
        latest.update(cx, |editor, cx| {
            editor.input.update(cx, |input, cx| input.set_value("Latest notebook note", window, cx));
            editor.finish(Completion::OpenReader, window, cx);
        })
    });
    assert_eq!(opened.get(), 0, "navigation must wait for the save acknowledgement");
    visual.condition(&latest, |editor, _| matches!(editor.state, EditorState::Closed)).await;
    assert_eq!(opened.get(), 1);
    let saved = backend.annotations(id).await.unwrap().remove(0);
    assert_eq!(saved.note, "Latest notebook note");

    // Backend validation supplies a real failing write without relying on
    // conflicts. The failed draft and requested navigation must survive retry.
    let failed = visual.update(|window, cx| show_editor(library.clone(), book.title.clone(), saved.clone(), window, cx));
    visual.update(|window, cx| {
        failed.update(cx, |editor, cx| {
            editor.save_annotation_changes(None, Some(0x1000000), cx);
            editor.input.update(cx, |input, cx| input.set_value("Keep this failed draft", window, cx));
            editor.finish(Completion::OpenReader, window, cx);
        })
    });
    visual.condition(&failed, |editor, _| matches!(editor.state, EditorState::Failed { .. })).await;
    failed.read_with(visual, |editor, cx| {
        assert_eq!(editor.saved.note, saved.note);
        assert_eq!(editor.input.read(cx).value().as_str(), "Keep this failed draft");
    });
    visual.run_until_parked();
    visual.simulate_keystrokes("escape");
    visual.condition(&failed, |editor, _| matches!(editor.state, EditorState::Failed { .. })).await;
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(opened.get(), 1, "a failed save must not open the reader");

    visual.update(|window, cx| failed.update(cx, |editor, cx| editor.finish(Completion::OpenReader, window, cx)));
    visual.condition(&failed, |editor, _| matches!(editor.state, EditorState::Failed { .. })).await;
    visual.update(|window, cx| {
        failed.update(cx, |editor, cx| {
            editor.draft.color = saved.color;
            editor.retry(window, cx);
        })
    });
    assert_eq!(opened.get(), 1);
    visual.condition(&failed, |editor, _| matches!(editor.state, EditorState::Closed)).await;
    assert_eq!(opened.get(), 2);
    let persisted = backend.annotations(id).await.unwrap().remove(0);
    assert_eq!(persisted.note, "Keep this failed draft");

    let unchanged = visual.update(|window, cx| show_editor(library.clone(), book.title.clone(), persisted.clone(), window, cx));
    visual.update(|window, cx| {
        unchanged.update(cx, |editor, cx| {
            editor.finish(Completion::Dismiss, window, cx);
            assert!(matches!(editor.state, EditorState::Closed), "unchanged dismissal must finish synchronously");
        })
    });
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));

    let deleted = visual.update(|window, cx| show_editor(library, book.title, persisted, window, cx));
    visual.update(|window, cx| deleted.update(cx, |editor, cx| editor.delete_edited_annotation(window, cx)));
    visual.condition(&deleted, |editor, _| matches!(editor.state, EditorState::Closed)).await;
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(backend.annotations(id).await.unwrap().is_empty());
}
