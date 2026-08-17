/** Convert publication-root EPUB paths from the backend to Foliate resource hrefs. */
export function normalizeFoliateTarget(target: string): string {
  if (target.startsWith("/") && !target.startsWith("//")) return target.slice(1);
  return target;
}
