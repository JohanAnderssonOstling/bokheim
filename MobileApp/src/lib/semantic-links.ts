import type { BookLink } from "./types";

function randomUuid() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export function createBookLink(sourceBookId: string, targetBookId: string, relation: string, note: string, now = Date.now()): BookLink {
  return {
    id: randomUuid(),
    source_book_id: sourceBookId,
    target_book_id: targetBookId,
    relation: relation.trim(),
    note: note.trim(),
    created_at: now,
    modified_at: now,
  };
}

export function reviseBookLink(link: BookLink, relation: string, note: string, now = Date.now()): BookLink {
  return { ...link, relation: relation.trim(), note: note.trim(), modified_at: Math.max(now, link.created_at) };
}
