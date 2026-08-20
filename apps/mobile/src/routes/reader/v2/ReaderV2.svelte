<script>
  import { onMount, tick } from "svelte";
  import { page } from "$app/state";
  import { convertFileSrc } from "@tauri-apps/api/core";
  // Registers the <foliate-view> custom element.
  import "../../../vendor/foliate-js/view.js";
  import { commands } from "../../../lib/app/commands";
  import { isWebApp } from "../../../lib/app/platform";
  import TopBar from "./TopBar.svelte";
  import Sidebar from "./Sidebar.svelte";
  import TocPanel from "./TocPanel.svelte";
  import SearchBar from "./SearchBar.svelte";
  import AnnotationsPanel from "./AnnotationsPanel.svelte";
  import BookmarkPanel from "./BookmarkPanel.svelte";
  import { settings } from "./settings.svelte";
  import { createAnnotations, drawOverlay, overlayOf } from "./annotations.svelte";
  import { createBookmarks } from "./bookmarks.svelte";
  import { elementRect, placeAnnotationPopup, rangeRect } from "./placement";
  import { createFootnotes } from "./footnotes.svelte";
  import AnnotationEditor from "../../../components/shared/readers/annotations/AnnotationEditor.svelte";
  import { readerStyles } from "./styles";
  import { preferences } from "../../../lib/app/preferences.svelte";
  import { resolveAppearance } from "../../../lib/app/application-appearance";
  import { flatten } from "./toc";
  import { attachSession } from "./session";
  import { openExternalUrl } from "../../../lib/app/external-links";
  import { goto } from "$app/navigation";
  import { hasOwnHistory } from "../../../lib/app/navigate";
  import { libraryRootHref } from "../../../lib/app/navigation";

  let libraryId = page.url.searchParams.get("library") ?? "";
  let bookId = page.params.bookId ?? "";

  let view = $state();
  let error = $state("");
  let sidebarOpen = $state(false);
  let tab = $state("contents");
  let chromeHidden = $state(false);
  let activeHref = $state("");
  let searchOpen = $state(false);
  let matches = $state([]);
  let selected = $state(-1);
  let searchRun = 0;
  /// `view.book` is not reactive, so the sidebar needs telling when to read it.
  let opened = $state(false);

  // One store for both readers of it: the highlights drawn in the text and the
  // list in the sidebar.
  let annotations = createAnnotations(libraryId, bookId);
  let bookmarks = createBookmarks(libraryId, bookId);

  /// Foliate drops overlays when a section re-renders, so they are re-added.
  function redrawOverlays() {
    for (let annotation of annotations.items) {
      let overlay = overlayOf(annotation);
      if (overlay) view.addAnnotation(overlay);
    }
  }

  function goToAnnotation(annotation) {
    let overlay = overlayOf(annotation);
    if (overlay) view.goTo(overlay.value);
    sidebarOpen = false;
  }

  function goToBookmark(bookmark) {
    view.goTo(bookmark.cfi);
    sidebarOpen = false;
  }

  /// The toc entry titling the current page, or a plain fallback for a book
  /// with none — mirroring what the section stepper already tracks.
  function currentLabel() {
    let entry = flatten(view.book?.toc).find((item) => item.href === activeHref);
    return entry?.label || `Section ${section + 1}`;
  }

  function addBookmark() {
    bookmarks.add(stored, currentLabel());
  }

  /// The annotation being edited, and where its popup sits.
  let editing = $state(null);
  let anchor = $state(null);
  /// Null until measured; the editor renders itself hidden while it is.
  let popup = $state(null);
  let popupElement = null;

  // Placement needs the popup's real size, and its height depends on the note,
  // so it can only be measured once rendered. The editor exposes its textarea;
  // the popup is what that sits in.
  async function noteMounted(node) {
    popupElement = node?.closest(".reader-annotation-editor") ?? null;
    await tick();
    if (!popupElement || !anchor) return;
    popup = placeAnnotationPopup(anchor, popupElement.getBoundingClientRect(), {
      width: window.innerWidth,
      height: window.innerHeight,
    });
  }

  /// What is selected right now, captured where it can still be read.
  let selection = $state(null);

  // Read at pointerup rather than when the highlight button is pressed: the
  // selection inside the iframe is gone by the time that click lands.
  function captureSelection(doc, index) {
    let live = doc.getSelection();
    if (!live || live.type !== "Range") {
      selection = null;
      return;
    }
    let range = live.getRangeAt(0);
    let text = live.toString().trim();
    let cfi = view.getCFI(index, range);
    selection = text && cfi ? { cfi, text, rect: rangeRect(range, doc) } : null;
  }

  // The annotation exists as soon as it is made; the editor only refines it.
  async function highlightSelection() {
    if (!selection) return;
    anchor = selection.rect;
    popup = null;
    let saved = await annotations.save(annotations.draft(selection.cfi, selection.text));
    selection = null;
    let overlay = overlayOf(saved);
    if (overlay) view.addAnnotation(overlay);
    editing = saved;
  }

  /// Tapping a highlight foliate already knows about reopens its editor.
  function showAnnotation(event) {
    let { value, range } = event.detail;
    let annotation = annotations.items.find((item) => overlayOf(item)?.value === value);
    if (!annotation) return;
    let doc = range?.startContainer.ownerDocument;
    if (range && doc) anchor = rangeRect(range, doc);
    popup = null;
    editing = annotation;
  }

  async function editAnnotation(changes) {
    if (!editing) return;
    editing = await annotations.save({ ...editing, ...changes });
    let overlay = overlayOf(editing);
    if (overlay) {
      // Redrawing is a delete and a re-add; foliate has no update.
      await view.deleteAnnotation(overlay);
      await view.addAnnotation(overlay);
    }
  }

  async function deleteEditing() {
    if (!editing) return;
    let overlay = overlayOf(editing);
    if (overlay) await view.deleteAnnotation(overlay);
    await annotations.remove(editing);
    closeEditor();
  }

  function closeEditor() {
    editing = null;
    anchor = null;
    popup = null;
    popupElement = null;
  }
  /// The last position written, so re-renders and resizes don't rewrite it.
  let stored = "";
  /// Where in the spine the page is. Not state: only the keys read it.
  let section = 0;
  let sectionCount = 0;

  /// Asks the same overlayer foliate hit-tests against, so both agree.
  function annotationAt(event, index) {
    let contents = view.renderer?.getContents?.() ?? [];
    let overlayer = contents.find((entry) => entry.index === index)?.overlayer;
    let [value] = overlayer?.hitTest(event) ?? [];
    return Boolean(value);
  }

  // Highlights are painted into an SVG overlay rather than being elements the
  // text can style, so the cursor has to be set from the same hit test.
  // `mousemove` rather than `pointermove`: there is no hovering on touch.
  function hover(event, doc, index) {
    let cursor = annotationAt(event, index) ? "pointer" : "";
    if (doc.documentElement.style.cursor !== cursor)
      doc.documentElement.style.cursor = cursor;
  }

  /**
   * The browser fires this after a drag-select too, and finishing a selection
   * is not a request to hide the chrome. Nor is tapping a highlight, which
   * foliate turns into `show-annotation` on this very click.
   */
  function tapped(event, doc, index) {
    if (doc.getSelection()?.type === "Range") return;
    if (annotationAt(event, index)) return;
    // A tap anywhere else is a request to put the editor away.
    if (editing) {
      closeEditor();
      return;
    }
    chromeHidden = !chromeHidden;
  }

  /// The top bar: opens the sidebar onto a panel, or closes it if already up.
  function showPanel(next) {
    if (sidebarOpen && tab === next) {
      sidebarOpen = false;
      return;
    }
    tab = next;
    sidebarOpen = true;
  }

  /// The sidebar's own tabs, which switch panels without closing anything.
  function selectPanel(next) {
    tab = next;
  }

  // Foliate reports a plain string unless the publication gives the title in
  // more than one language, in which case it is a map keyed by language.
  function titleOf(book) {
    let title = book?.metadata?.title;
    if (!title) return "";
    return typeof title === "string" ? title : (Object.values(title)[0] ?? "");
  }

  function goTo(href) {
    view.goTo(href);
    sidebarOpen = false;
  }

  // Foliate streams results as it works through the book, so a second search
  // would interleave with the first. Runs are numbered and a stale run drops
  // whatever it is still yielding.
  async function runSearch(query) {
    let current = ++searchRun;
    matches = [];
    selected = -1;
    view.clearSearch();
    let wanted = query.trim();
    if (!wanted) return;

    for await (let result of view.search({ query: wanted })) {
      if (current !== searchRun) return;
      // Foliate reports progress and section labels on the same channel.
      if (typeof result === "string" || "progress" in result) continue;
      matches = "subitems" in result ? [...matches, ...result.subitems] : [...matches, result];
    }
  }

  /// Steps through matches, wrapping at both ends. `select` is what highlights.
  function cycle(offset) {
    if (!matches.length) return;
    selected =
      selected < 0
        ? offset > 0
          ? 0
          : matches.length - 1
        : (selected + offset + matches.length) % matches.length;
    view.select(matches[selected].cfi);
  }

  function toggleSearch() {
    searchOpen = !searchOpen;
    // Leaving search should take its highlights with it.
    if (!searchOpen) {
      searchRun += 1;
      matches = [];
      selected = -1;
      view.clearSearch();
    }
  }

  async function show() {
    try {
      // Resolves the book on disk and lets the WebView read that one file.
      let path = await commands.openBook(libraryId, bookId);
      stored = (await commands.getBookReadingPosition(libraryId, bookId)) ?? "";
      await view.open(isWebApp ? path : convertFileSrc(path));
      await view.init({ lastLocation: stored || undefined, showTextStart: !stored });
      opened = true;
      // The renderer only exists once the book is open, so the effect that
      // tracks changes cannot have applied anything yet.
      style();
      layout();
      // Highlights enrich a page that already reads, so this does not hold up
      // the open.
      annotations.restore().then(redrawOverlays);
      bookmarks.restore();
    } catch (reason) {
      error = String(reason);
    }
  }

  // Foliate injects this into every section document, including ones loaded
  // later, so it does not need reapplying per section.
  //
  // The stylesheet is built before the renderer is reached: optional chaining
  // would skip evaluating the argument while the renderer is still missing, so
  // the effect below would read no settings and never run again.
  // Reads the application theme as well as the reader's own settings, so the
  // effect below re-applies when either changes.
  function currentStyles() {
    return readerStyles(
      settings,
      preferences.browsing.theme,
      resolveAppearance(preferences.browsing.appearance),
    );
  }

  /**
   * Column count is never chosen, only the column width is.
   *
   * Foliate lays out `min(maxColumnCount, ceil(width / maxInlineSize))`
   * columns, so a cap high enough never to bind leaves the width as the only
   * thing deciding how many fit. Portrait still gets one column, which foliate
   * enforces itself.
   */
  const UNCAPPED_COLUMNS = "99";

  /**
   * The gutter, as foliate wants it: a percentage of the whole column area,
   * subtracted once per column. It compounds with the column count — at 7% and
   * four columns nearly a third of the width is gutter — so with the count
   * uncapped it is set lower than foliate's default.
   */
  const COLUMN_GAP_PERCENT = "4%";

  // `setAttribute` re-fires the paginator's `attributeChangedCallback` even for
  // an unchanged value, and a flow or width change there relays the whole
  // chapter — so unchanged attributes are left alone.
  function attribute(renderer, name, value) {
    if (renderer.getAttribute(name) !== value) renderer.setAttribute(name, value);
  }

  // The width is read before the renderer is reached: returning early first
  // would leave the effect below with no settings to track, so it would never
  // run again.
  function layout() {
    let width = `${settings.max_line_width_px}px`;
    let renderer = view?.renderer;
    if (!renderer) return;
    // Count first: foliate only sets a custom property for it, while the width
    // also relays the chapter — so the width has to be the one applied last.
    attribute(renderer, "max-column-count", UNCAPPED_COLUMNS);
    attribute(renderer, "gap", COLUMN_GAP_PERCENT);
    attribute(renderer, "max-inline-size", width);
  }

  function style() {
    let css = currentStyles();
    view?.renderer?.setStyles?.(css);
  }

  $effect(style);
  $effect(layout);

  async function store(cfi, fraction) {
    try {
      // Progress is a percentage: the library's progress bars read it directly.
      await commands.updateReadingPosition(libraryId, bookId, cfi, fraction * 100);
    } catch {
      // A lost write costs the reader their place, not the reading session.
    }
  }

  // Written on every page turn rather than debounced, so there is never an
  // unsaved position for a backgrounded app to lose.
  function relocate(event) {
    // Which contents entry the page belongs to, so the panel can mark it.
    activeHref = event.detail.tocItem?.href ?? activeHref;
    // Where in the spine it is, which is what chapter stepping moves through.
    section = event.detail.section?.current ?? section;
    sectionCount = event.detail.section?.total ?? sectionCount;
    let cfi = event.detail.cfi;
    if (!cfi || cfi === stored) return;
    stored = cfi;
    store(cfi, event.detail.fraction ?? 0);
  }


  /// Steps whole sections, clamped to the ends of the book.
  function goToSection(offset) {
    if (!sectionCount) return;
    let target = Math.max(0, Math.min(sectionCount - 1, section + offset));
    if (target !== section) view.goTo(target);
  }

  // Ctrl-arrows move by chapter, plain arrows by page, matching the shortcut
  // contract's `primary_modifier` on `previous_section` and `next_section`.
  function turn(event) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    let back = event.key === "ArrowLeft";
    event.preventDefault();
    if (event.ctrlKey || event.metaKey) goToSection(back ? -1 : 1);
    else if (back) view.goLeft();
    else view.goRight();
  }

  // Each section renders in its own iframe, so keys pressed while reading never
  // reach this document. The listener dies with the iframe.


  let footnotes = createFootnotes(function (renderer) {
    renderer?.setStyles?.(currentStyles());
  });

  /// Where the note sits, and whether it is showing.
  let footnoteAnchor = $state(null);
  let footnoteHost = $state();

  let footnotePopup = $derived(
    footnoteAnchor && footnotes.element
      ? placeAnnotationPopup(
          footnoteAnchor,
          { width: FOOTNOTE.width, height: FOOTNOTE.height },
          { width: window.innerWidth, height: window.innerHeight },
        )
      : null,
  );

  const FOOTNOTE = { width: 360, height: 260 };

  // Modifier state is not in the link event, and reading it here works because
  // a capture listener on the same document runs earlier in the same dispatch.
  let modifierHeld = false;

  /**
   * Ctrl-clicking a footnote reference opens the note in place; a plain click
   * still jumps to it, because an unprevented `link` lets foliate navigate.
   *
   * `handle` has to run inside the dispatch: it is what calls `preventDefault`.
   */
  function link(event) {
    if (!modifierHeld) return;
    let anchor = event.detail?.a;
    let doc = anchor?.ownerDocument;
    footnoteAnchor = anchor && doc ? elementRect(anchor, doc) : null;
    let opening = footnotes.open(view.book, event);
    // Undefined means it was not a footnote reference, so foliate navigates.
    if (!opening) footnoteAnchor = null;
    else opening.catch((reason) => console.warn("Could not open footnote", reason));
  }

  function closeFootnote() {
    footnotes.close();
    footnoteAnchor = null;
  }

  // The popup is mounted only once the note exists, so it is attached here
  // rather than by whoever created it.
  $effect(function () {
    if (footnoteHost && footnotes.element) footnoteHost.replaceChildren(footnotes.element);
  });

  /**
   * Left alone, foliate would `window.open` an external link, which in a Tauri
   * WebView opens it inside the app. Preventing the default claims it.
   *
   * Internal links are deliberately not handled: unprevented, foliate navigates
   * to them itself, which is what should happen.
   */
  async function externalLink(event) {
    event.preventDefault();
    let href = event.detail?.href_;
    if (!href) return;
    try {
      await openExternalUrl(href);
    } catch (reason) {
      console.warn("Could not open external link", reason);
    }
  }

  /**
   * Leaves the reader the way it was arrived at, when that is possible.
   *
   * Only pops when the application itself pushed the entry behind this one.
   * A reader reached by deep link has the previous website behind it, and a
   * reader restored on a cold start may have nothing at all — popping either
   * would leave the application rather than return to the library.
   */
  function close() {
    if (hasOwnHistory()) {
      history.back();
      return;
    }
    void goto(libraryId ? libraryRootHref(libraryId) : "/library", { replaceState: true });
  }

  /// Android's volume keys, which the system reports rather than the document.
  function volumeTurn(direction) {
    if (direction === "next") view.goRight();
    else view.goLeft();
  }

  onMount(function () {
    addEventListeners();
    let detachSession = attachSession(volumeTurn);
    show();

    return function () {
      window.removeEventListener("keydown", turn);
      detachSession();
    };
  });
  function addEventListeners() {
    function section (event) {
      let doc = event.detail.doc;
      let index = event.detail.index;
      doc.addEventListener("keydown", turn);
      // Capture, so it records the modifier before foliate's own link handler
      // runs on the same click.
      doc.addEventListener(
        "click",
        (event) => (modifierHeld = event.ctrlKey || event.metaKey),
        true,
      );
      doc.addEventListener("click", (event) => tapped(event, doc, index));
      doc.addEventListener("mousemove", (event) => hover(event, doc, index));
      // The selection is not final until after this event has been handled.
      doc.addEventListener("pointerup", () => setTimeout(() => captureSelection(doc, index)));
    }
    view.addEventListener("load", (section));
    view.addEventListener("relocate", relocate);
    view.addEventListener("draw-annotation", drawOverlay);
    view.addEventListener("create-overlay", redrawOverlays);
    view.addEventListener("show-annotation", showAnnotation);
    view.addEventListener("external-link", externalLink);
    view.addEventListener("link", link);
    window.addEventListener("keydown", turn);
  }
</script>

<div class="reader">
  <TopBar
    title={opened ? titleOf(view.book) : ""}
    panel={sidebarOpen ? tab : ""}
    {searchOpen}
    onClose={close}
    onContents={() => showPanel("contents")}
    onSearch={toggleSearch}
    hidden={chromeHidden}
  />

  <div
    class="reader-body"
    class:has-sidebar={sidebarOpen}
  >
    <Sidebar
      {tab}
      onTab={selectPanel}
      hidden={!sidebarOpen}
    >
      {#snippet contents()}
        <TocPanel
          items={opened ? flatten(view.book?.toc) : []}
          {activeHref}
          onSelect={goTo}
        />
      {/snippet}

      {#snippet annotationList()}
        <AnnotationsPanel
          items={annotations.items}
          onSelect={goToAnnotation}
          onDelete={annotations.remove}
        />
      {/snippet}

      {#snippet bookmarkList()}
        <BookmarkPanel
          {bookmarks}
          bookmarked={bookmarks.items.some((item) => item.cfi === stored)}
          onAdd={addBookmark}
          onSelect={goToBookmark}
        />
      {/snippet}
    </Sidebar>

    <foliate-view bind:this={view}></foliate-view>
  </div>

  {#if footnotes.element && footnotePopup}
    <aside
      class="footnote"
      style="left: {footnotePopup.x}px; top: {footnotePopup.y}px; width: {FOOTNOTE.width}px; height: {FOOTNOTE.height}px"
    >
      <button
        class="footnote-close"
        aria-label="Close note"
        onclick={closeFootnote}>×</button
      >
      <div
        class="footnote-body"
        bind:this={footnoteHost}
      ></div>
    </aside>
  {/if}

  {#if selection?.rect}
    <button
      class="highlight-selection"
      style="left: {(selection.rect.left + selection.rect.right) / 2}px; top: {selection.rect
        .bottom}px"
      onclick={highlightSelection}>Highlight</button
    >
  {/if}

  {#if editing}
    <!-- Keyed so a different annotation remounts the textarea, which is what
         reports the popup back for measuring. -->
    {#key editing.id}
      <AnnotationEditor
      annotation={editing}
      note={editing.note}
      position={popup}
      placement={popup?.placement}
      onNoteChange={(note) => editAnnotation({ note })}
      onStyleChange={(next) => editAnnotation({ style: next })}
      onColorChange={(next) => editAnnotation({ color: next })}
      onDelete={deleteEditing}
      onClose={closeEditor}
      onTextareaMount={noteMounted}
        showDelete
        showClose
      />
    {/key}
  {/if}

  {#if searchOpen && !chromeHidden}
    <SearchBar
      count={matches.length}
      index={selected}
      onSearch={runSearch}
      onCycle={cycle}
    />
  {/if}

  {#if error}
    <p class="reader-error">{error}</p>
  {/if}
</div>

<style>
  /* Flex, not grid: hiding the chrome removes a child, and a fixed row template
     would then put the book in the `auto` row and collapse it to nothing. */
  .reader {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* Takes whatever the bar leaves. min-height lets the book shrink, so the
     paginator repaginates instead of growing the page. Positioned, because the
     sidebar overlays it. */
  .reader-body {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* Room to spare: inset the book so the sidebar sits beside it and the columns
     repaginate. Narrow screens have no width to give, so it overlays there. */
  @media (min-width: 721px) {
    .reader-body.has-sidebar foliate-view {
      margin-inline-start: min(380px, 40vw);
    }
  }

  /* min-width lets the book shrink when the sidebar opens, so the paginator
     repaginates instead of overflowing. */
  foliate-view {
    display: block;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .reader-error {
    flex: none;
    padding: 1rem;
  }

  .footnote {
    position: fixed;
    z-index: 20;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    box-sizing: border-box;
    padding: 6px;
    background: var(--color-surface-bg);
    border: 1px solid var(--color-border);
    box-shadow: 0 1rem 3rem color-mix(in srgb, var(--color-ink-text) 24%, transparent);
  }

  .footnote-close {
    justify-self: end;
    padding: 0 6px;
    background: none;
    border: 0;
    color: var(--color-subtle-text);
    cursor: pointer;
  }

  /* The note renders as its own foliate view, which needs a definite box. */
  .footnote-body {
    min-height: 0;
  }

  .footnote-body :global(foliate-view) {
    display: block;
    width: 100%;
    height: 100%;
  }

  /* Sits over the text at the end of the selection it would highlight. */
  .highlight-selection {
    position: fixed;
    z-index: 8;
    translate: -50% 4px;
    padding: 6px 10px;
    background: var(--color-raised-bg);
    border: 1px solid var(--color-border);
    color: inherit;
    cursor: pointer;
  }
</style>
