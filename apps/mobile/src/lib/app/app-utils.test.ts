import assert from "node:assert/strict";
import test from "node:test";

import {
  persistableSession,
  routeFromSession,
  sessionFromRoute,
  type BrowserSession,
} from "./app-utils.ts";
import type { LibraryFeature } from "../bindings.ts";
import type { LibraryPageLevel } from "../library/page-types.ts";
import { defaultLibraryPlace, type LibraryPlace } from "../session/ui-session.ts";

const LIBRARY = "11111111-1111-4111-8111-111111111111";

/**
 * Mirrors BrowserSessionStore's contract without its runes, so the derivation
 * under test can be exercised outside a Svelte component.
 */
function recordingSession(): BrowserSession & { place(id: string): LibraryPlace } {
  const places: Record<string, LibraryPlace> = {};
  return {
    place: (libraryId) => places[libraryId] ?? defaultLibraryPlace(),
    remember(
      libraryId: string,
      route: LibraryFeature,
      folderPath: LibraryPageLevel[],
      subjectPath: LibraryPageLevel[],
    ) {
      const previous = places[libraryId] ?? defaultLibraryPlace();
      places[libraryId] = {
        route,
        folder_path: route === "folders" ? [...folderPath] : [...previous.folder_path],
        subject_path:
          route === "subjects" ? [...subjectPath] : [...previous.subject_path],
      };
    },
    snapshot: (activeLibraryId?: string) => ({
      kind: "browse",
      active_library_id: activeLibraryId || undefined,
      places,
    }),
  };
}

test("a folder route records the folder it is actually showing", () => {
  const browserSession = recordingSession();
  const result = sessionFromRoute({
    route: {
      id: "/library/[libraryId]/folder/[folderId]",
      libraryId: LIBRARY,
      folderId: "fiction",
    },
    activeLibraryId: LIBRARY,
    browserSession,
  });
  assert.equal(result?.kind, "browse");
  assert.deepEqual(browserSession.place(LIBRARY), {
    route: "folders",
    folder_path: [{ id: "fiction", name: "fiction" }],
    subject_path: [],
  });
});

test("the folder root is remembered as no folder at all", () => {
  const browserSession = recordingSession();
  sessionFromRoute({
    route: {
      id: "/library/[libraryId]/folder/[folderId]",
      libraryId: LIBRARY,
      folderId: "__root__",
    },
    activeLibraryId: LIBRARY,
    browserSession,
  });
  assert.deepEqual(browserSession.place(LIBRARY).folder_path, []);
});

test("a subject route records its subject without disturbing the folder path", () => {
  const browserSession = recordingSession();
  sessionFromRoute({
    route: {
      id: "/library/[libraryId]/folder/[folderId]",
      libraryId: LIBRARY,
      folderId: "fiction",
    },
    activeLibraryId: LIBRARY,
    browserSession,
  });
  sessionFromRoute({
    route: {
      id: "/library/[libraryId]/subjects/[subjectId]",
      libraryId: LIBRARY,
      subjectId: "history",
    },
    activeLibraryId: LIBRARY,
    browserSession,
  });
  const place = browserSession.place(LIBRARY);
  assert.equal(place.route, "subjects");
  assert.deepEqual(place.subject_path, [{ id: "history", name: "history" }]);
  assert.deepEqual(place.folder_path, [{ id: "fiction", name: "fiction" }]);
});

test("navigating between folders replaces the remembered folder", () => {
  const browserSession = recordingSession();
  for (const folderId of ["fiction", "poetry"])
    sessionFromRoute({
      route: {
        id: "/library/[libraryId]/folder/[folderId]",
        libraryId: LIBRARY,
        folderId,
      },
      activeLibraryId: LIBRARY,
      browserSession,
    });
  assert.deepEqual(browserSession.place(LIBRARY).folder_path, [
    { id: "poetry", name: "poetry" },
  ]);
});

test("a reader route reports the book instead of a browse place", () => {
  const contentHash = "a".repeat(64);
  const result = sessionFromRoute({
    route: {
      id: "/reader/epub/[bookId]",
      bookId: contentHash,
      readerLibraryId: LIBRARY,
    },
    activeLibraryId: LIBRARY,
    browserSession: recordingSession(),
  });
  assert.deepEqual(result, {
    kind: "reader",
    content_hash: contentHash,
    reader_kind: "epub",
    library_id: LIBRARY,
  });
});

test("V2 EPUB and PDF routes report their own reader kind", () => {
  const contentHash = "c".repeat(64);
  for (const readerKind of ["epub", "pdf"] as const) {
    const result = sessionFromRoute({
      route: {
        id: `/reader/v2/${readerKind}/[bookId]`,
        bookId: contentHash,
        readerLibraryId: LIBRARY,
      },
      activeLibraryId: LIBRARY,
      browserSession: recordingSession(),
    });
    assert.deepEqual(result, {
      kind: "reader",
      content_hash: contentHash,
      reader_kind: readerKind,
      library_id: LIBRARY,
    });
  }
});

test("a browse session reopens the route it was recorded from", () => {
  const cases = [
    ["/library/[libraryId]", {}, `/library/${LIBRARY}`],
    [
      "/library/[libraryId]/folder/[folderId]",
      { folderId: "fiction" },
      `/library/${LIBRARY}/folder/fiction`,
    ],
    [
      "/library/[libraryId]/subjects/[subjectId]",
      { subjectId: "history" },
      `/library/${LIBRARY}/subjects/history`,
    ],
    ["/library/[libraryId]/authors", {}, `/library/${LIBRARY}/authors`],
    ["/library/[libraryId]/trash", {}, `/library/${LIBRARY}/trash`],
  ] as const;
  for (const [id, params, expected] of cases) {
    // A fresh session per case: the store deliberately keeps the folder and
    // subject paths of routes it is not currently on.
    const session = sessionFromRoute({
      route: { id, libraryId: LIBRARY, ...params },
      activeLibraryId: LIBRARY,
      browserSession: recordingSession(),
    });
    assert.ok(session);
    assert.equal(routeFromSession(session), expected, `${id} did not round-trip`);
  }
});

test("a reader session round-trips through its route", () => {
  const contentHash = "b".repeat(64);
  for (const readerKind of ["epub", "pdf"] as const) {
    const session = sessionFromRoute({
      route: {
        id: `/reader/${readerKind}/[bookId]`,
        bookId: contentHash,
        readerLibraryId: LIBRARY,
      },
      activeLibraryId: LIBRARY,
      browserSession: recordingSession(),
    });
    assert.equal(
      routeFromSession(session!),
      `/reader/${readerKind}/${contentHash}?library=${LIBRARY}`,
    );
  }
});

test("a locally opened file is never persisted", () => {
  assert.equal(
    persistableSession({
      kind: "reader",
      content_hash: "foliate-position:book.epub:100:200",
      reader_kind: "epub",
    }),
    null,
  );
});
