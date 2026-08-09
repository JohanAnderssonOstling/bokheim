# Language-Aware Text Shaping

**Status:** Potential feature; deferred pending GPUI support

## Goal

Allow the shaping backend to select language-specific OpenType forms from the
inherited HTML `lang` or XHTML `xml:lang` value. This primarily affects fonts
that expose localized forms through features such as `locl`.

## Current blocker

The renderer delegates authoritative run shaping to GPUI's text system. Its
current run and line-layout interface does not expose a supported per-run
language value, so the HTML renderer cannot propagate language into the
backend shaper without bypassing or forking GPUI's shaping path.

Do not add language fields only to the HTML-side shaping contract while GPUI
cannot consume them. That would create apparent support without changing the
painted glyphs and would complicate cache identity prematurely.

## Activation condition

Resume this work when GPUI provides all of the following:

- A per-run or range-based language field accepted by authoritative shaping.
- BCP 47 language tags, or a documented mapping from them to the backend's
  language representation.
- Stable cluster, advance, and glyph output through the same layout and paint
  path already used by the reader.
- Language included in GPUI's relevant shaping-cache identity.

## Proposed implementation

1. Resolve inherited `lang` and `xml:lang` during document preparation.
2. Normalize valid BCP 47 tags while retaining a safe unspecified-language
   fallback for missing or invalid values.
3. Split shaping runs at effective-language boundaries.
4. Carry the language through prepared styles and document shaping requests.
5. Set the corresponding GPUI run-language field.
6. Include language in HTML-side shaped-run and authoritative-registration
   cache identities.
7. Treat a language-only DOM/style change as a shaping invalidation without
   unnecessarily rebuilding unrelated resources.

## Invariants

- Language remains inherited according to HTML/XML semantics.
- A nested language change affects only its descendant text.
- Missing, empty, or invalid language tags never make text disappear.
- Selection, hit testing, links, and character-to-glyph mapping remain stable
  when localized forms change glyph IDs or advances.
- Web-compatible and book-optimized modes use identical language shaping.
- The GPUI path remains the single authoritative source of shaped glyphs.

## Tests

- HTML `lang` inheritance from the document element.
- XHTML `xml:lang` inheritance.
- A nested language override and restoration to the parent language.
- Multiple language tokens within one paragraph produce separate shaping runs.
- Missing and malformed tags use the unspecified-language fallback.
- A deterministic adapter captures the language supplied for each run.
- A font with known `locl` forms demonstrates different glyph selection for
  two supported languages.
- Language participates in cache reuse and invalidation tests.
- Selection and hit testing remain correct across language boundaries.

## Non-goals

- Maintaining a separate Rustybuzz shaping pipeline beside GPUI.
- Guessing language from script or text content.
- Transliteration, locale-sensitive case conversion, or hyphenation policy.
- Shipping an HTML-only language field that the active backend ignores.

## Acceptance criterion

Ship only when setting `lang` or `xml:lang` can demonstrably change localized
glyph selection through GPUI's normal shaping and painting path without
breaking cache correctness or interaction geometry.
