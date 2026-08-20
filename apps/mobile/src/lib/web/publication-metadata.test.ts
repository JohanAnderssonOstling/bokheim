import assert from "node:assert/strict";
import test from "node:test";

import { inspectPublication } from "./publication-metadata.ts";
import { storedZip } from "./test-epub.ts";

const tinyPng = Uint8Array.from(
  Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  ),
);

test("the shared Rust EPUB provider inspects a real archive", async () => {
  const bytes = storedZip({
    mimetype: "application/epub+zip",
    "META-INF/container.xml":
      '<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "OPS/book.opf":
      '<package><metadata><dc:title>Archive Title</dc:title><dc:creator>Archive Author</dc:creator><dc:identifier>9780306406157</dc:identifier><dc:description>Archive description</dc:description><dc:subject>History</dc:subject><dc:subject>United States</dc:subject><dc:subject>20th Century</dc:subject></metadata><manifest><item id="cover" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/></manifest></package>',
    "OPS/images/cover.jpg": tinyPng,
  });
  const file = new File([bytes], "archive.epub", { type: "application/epub+zip" });
  const metadata = await inspectPublication(file, file.name, bytes);
  const { cover, ...fields } = metadata;
  assert.deepEqual(fields, {
    title: "Archive Title",
    author: "Archive Author",
    description: "Archive description",
    subjects: ["History", "United States", "20th Century"],
    subject_paths: ["Humanities / History"],
    subject_codes: [{ system: "bisac", code: "HIS036060" }],
    isbns: ["9780306406157"],
    toc: [],
  });
  assert.equal(cover?.[0], 0xff);
  assert.equal(cover?.[1], 0xd8);
  assert.equal(cover?.at(-2), 0xff);
  assert.equal(cover?.at(-1), 0xd9);
});

test("the web publication boundary accepts PDF documents", async () => {
  const bytes = new TextEncoder().encode("%PDF-1.7\n%%EOF");
  const metadata = await inspectPublication(
    new File([bytes], "book.pdf", { type: "application/pdf" }),
    "book.pdf",
    bytes,
  );
  assert.deepEqual(metadata, {
    title: null,
    author: null,
    description: null,
    subjects: [],
    subject_paths: [],
    subject_codes: [],
    isbns: [],
    cover: null,
    toc: [],
  });
});

test("the web publication boundary rejects files with a false PDF extension", async () => {
  const bytes = new TextEncoder().encode("not a PDF");
  await assert.rejects(
    inspectPublication(new File([bytes], "book.pdf"), "book.pdf", bytes),
    /not a valid PDF/,
  );
});
