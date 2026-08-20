import type { LibraryPageLevel } from "../../../../../lib/library/page-types";

export function subjectBreadcrumb(subjectId: string): LibraryPageLevel[] {
  const names = subjectId
    .split(" / ")
    .map((name) => name.trim())
    .filter(Boolean);
  return names.map((name, index) => ({
    id: names.slice(0, index + 1).join(" / "),
    name,
  }));
}
