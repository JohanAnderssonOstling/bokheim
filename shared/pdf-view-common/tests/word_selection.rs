use pdf_view_common::{NormalizedRect, PageTextGeometry, SelectionGranularity, SelectionOptions, TextCharacter};

fn geometry(text: &str) -> PageTextGeometry {
    PageTextGeometry::new(text.chars().enumerate().map(|(index, character)| TextCharacter::new(index * 2, character, NormalizedRect::new(index as f32 * 0.01, 0.1, 0.009, 0.02))))
}

fn point(index: usize) -> (f32, f32) {
    (index as f32 * 0.01 + 0.004, 0.11)
}

#[test]
fn ordinary_selection_snaps_both_ends_in_either_direction() {
    let page = geometry("alpha beta gamma");
    for (anchor, focus) in [(2, 8), (8, 2)] {
        let selection = page.select(point(anchor), point(focus)).unwrap();
        assert_eq!(selection.text(), "alpha beta");
        assert_eq!(selection.first_source_index(), 0);
        assert_eq!(selection.last_source_index(), 18);
    }
    assert_eq!(page.select(point(2), point(2)).unwrap().text(), "alpha");
    assert_eq!(page.select(point(2), point(3)).unwrap().text(), "alpha");
    assert_eq!(SelectionGranularity::default(), SelectionGranularity::Word);
    assert_eq!(SelectionOptions::default().granularity(), SelectionGranularity::Word);
}

#[test]
fn unicode_words_and_graphemes_are_never_split() {
    for (text, expected) in [("cafe\u{301}", "cafe\u{301}"), ("don't", "don't"), ("l’été", "l’été"), ("👩‍👧", "👩‍👧"), ("🇸🇪", "🇸🇪")] {
        let page = geometry(text);
        for index in 0..text.chars().count() {
            assert_eq!(page.select(point(index), point(index)).unwrap().text(), expected, "{text:?} at {index}");
        }
    }
    let page = geometry("猫犬");
    assert_eq!(page.select(point(0), point(0)).unwrap().text(), "猫");
    assert_eq!(page.select(point(1), point(1)).unwrap().text(), "犬");
    let page = geometry("alpha,beta");
    assert_eq!(page.select(point(5), point(5)).unwrap().text(), ",");
}

#[test]
fn whitespace_snaps_to_words_and_blank_pages_have_no_word_selection() {
    let page = geometry("alpha   beta");
    for (index, expected) in [(5, "alpha"), (7, "beta")] {
        assert_eq!(page.select(point(index), point(index)).unwrap().text(), expected);
    }
    let selected = page.select(point(5), point(9)).unwrap();
    assert_eq!(selected.text(), "alpha   beta");
    assert_eq!(selected.first_source_index(), 0);
    assert_eq!(selected.last_source_index(), 22);
    assert!(geometry("   ").select(point(1), point(1)).is_none());
    assert_eq!(geometry("  alpha  ").select(point(1), point(1)).unwrap().text(), "alpha");
}

