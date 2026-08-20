import type {
  FoliateTocItem,
  ReaderAnnotation,
  ReaderNavigationKey,
} from "../../../../lib/shared/types";
import type { TocItem } from "./reader-types";

export function flattenToc(
  items: FoliateTocItem[] = [],
  depth = 0,
  parentKeys: string[] = [],
  result: TocItem[] = [],
) {
  for (const [index, item] of items.entries()) {
    const key =
      item.href ||
      `${parentKeys.join("/") || "root"}/${item.id ?? index}-${item.label || "section"}`;
    if (item.label) {
      result.push({
        key,
        label: item.label,
        href: item.href || undefined,
        depth,
        parentKeys,
        hasChildren: Boolean(item.subitems?.length),
      });
    }
    if (item.subitems?.length)
      flattenToc(item.subitems, depth + 1, [...parentKeys, key], result);
  }
  return result;
}

export function annotationCfi(annotation: ReaderAnnotation) {
  return annotation.anchor.kind === "epub_cfi" ? annotation.anchor.cfi : "";
}

export function annotationOverlay(annotation: ReaderAnnotation) {
  const value = annotationCfi(annotation);
  if (!value.startsWith("epubcfi(")) return null;
  return { value, color: annotation.color, style: annotation.style, id: annotation.id };
}

export function isTextEntryTarget(target: EventTarget | null) {
  const element = target as (Element & { closest?: Element["closest"] }) | null;
  return Boolean(
    element &&
    typeof element.closest === "function" &&
    element.closest("input, textarea, select, [contenteditable='true']"),
  );
}

export function navigationKey(key: string): ReaderNavigationKey | null {
  if (key === "ArrowUp") return "arrow_up";
  if (key === "ArrowDown") return "arrow_down";
  if (key === "ArrowLeft") return "arrow_left";
  if (key === "ArrowRight") return "arrow_right";
  if (key === "PageUp") return "page_up";
  if (key === "PageDown") return "page_down";
  if (key === " " || key === "Spacebar") return "space";
  return null;
}
