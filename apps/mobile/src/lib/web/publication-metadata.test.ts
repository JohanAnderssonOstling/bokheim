import assert from "node:assert/strict";
import test from "node:test";

import { inspectPublication } from "./publication-metadata.ts";

function crc32(bytes: Uint8Array): number {
  let value = 0xffffffff;
  for (const byte of bytes) {
    value ^= byte;
    for (let bit = 0; bit < 8; bit++) value = (value >>> 1) ^ (0xedb88320 & -(value & 1));
  }
  return (value ^ 0xffffffff) >>> 0;
}

function storedZip(files: Record<string, string>): Uint8Array {
  const encoder = new TextEncoder();
  const chunks: Uint8Array[] = [];
  const central: Uint8Array[] = [];
  let offset = 0;
  const record = (size: number, write: (view: DataView) => void) => {
    const bytes = new Uint8Array(size);
    write(new DataView(bytes.buffer));
    return bytes;
  };
  for (const [name, contents] of Object.entries(files)) {
    const nameBytes = encoder.encode(name);
    const data = encoder.encode(contents);
    const checksum = crc32(data);
    const local = record(30, (view) => {
      view.setUint32(0, 0x04034b50, true);
      view.setUint16(4, 20, true);
      view.setUint32(14, checksum, true);
      view.setUint32(18, data.length, true);
      view.setUint32(22, data.length, true);
      view.setUint16(26, nameBytes.length, true);
    });
    const directory = record(46, (view) => {
      view.setUint32(0, 0x02014b50, true);
      view.setUint16(4, 20, true);
      view.setUint16(6, 20, true);
      view.setUint32(16, checksum, true);
      view.setUint32(20, data.length, true);
      view.setUint32(24, data.length, true);
      view.setUint16(28, nameBytes.length, true);
      view.setUint32(42, offset, true);
    });
    chunks.push(local, nameBytes, data);
    central.push(directory, nameBytes);
    offset += local.length + nameBytes.length + data.length;
  }
  const centralSize = central.reduce((sum, bytes) => sum + bytes.length, 0);
  const end = record(22, (view) => {
    view.setUint32(0, 0x06054b50, true);
    view.setUint16(8, Object.keys(files).length, true);
    view.setUint16(10, Object.keys(files).length, true);
    view.setUint32(12, centralSize, true);
    view.setUint32(16, offset, true);
  });
  const result = new Uint8Array(offset + centralSize + end.length);
  let cursor = 0;
  for (const bytes of [...chunks, ...central, end]) {
    result.set(bytes, cursor);
    cursor += bytes.length;
  }
  return result;
}

test("the shared Rust EPUB provider inspects a real archive", async () => {
  const bytes = storedZip({
    mimetype: "application/epub+zip",
    "META-INF/container.xml": '<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "OPS/book.opf": '<package><metadata><dc:title>Archive Title</dc:title><dc:creator>Archive Author</dc:creator></metadata><manifest><item id="cover" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/></manifest></package>',
    "OPS/images/cover.jpg": "cover bytes",
  });
  const file = new File([bytes], "archive.epub", { type: "application/epub+zip" });
  assert.deepEqual(await inspectPublication(file, file.name, bytes), {
    title: "Archive Title",
    author: "Archive Author",
    cover: new TextEncoder().encode("cover bytes"),
  });
});
