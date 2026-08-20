/// A navigation entry flattened out of a tree, keeping its place in it.
export interface ReaderTocPanelItem<TData = unknown> {
  key: string;
  label: string;
  depth: number;
  disabled?: boolean;
  hasChildren?: boolean;
  expanded?: boolean;
  data?: TData;
}

export interface HierarchicalNavigationItem {
  key: string;
  label: string;
  depth: number;
  /// Keys of every ancestor, so visibility is a lookup rather than a tree walk.
  parentKeys: string[];
  hasChildren: boolean;
  disabled?: boolean;
}

export interface TocPanelItem<T> {
  key: string;
  label: string;
  depth: number;
  disabled?: boolean;
  hasChildren: boolean;
  expanded: boolean;
  data: T;
}

/**
 * Selects the entries a collapsible navigation list should currently show.
 *
 * The list is stored flat and rendered flat — depth is an indent, not nesting —
 * so an entry is visible exactly when every one of its ancestors is expanded.
 */
export function tocPanelItems<T extends HierarchicalNavigationItem>(
  items: T[],
  expandedKeys: Set<string>,
): Array<TocPanelItem<T>> {
  return items
    .filter((item) => item.parentKeys.every((key) => expandedKeys.has(key)))
    .map((item) => ({
      key: item.key,
      label: item.label,
      depth: item.depth,
      disabled: item.disabled,
      hasChildren: item.hasChildren,
      expanded: expandedKeys.has(item.key),
      data: item,
    }));
}

/// Expands the top level, so a freshly opened book shows its main divisions.
export function initialExpandedKeys(items: HierarchicalNavigationItem[]) {
  return new Set(
    items.filter((item) => item.depth === 0 && item.hasChildren).map((item) => item.key),
  );
}

/// Flips one entry between expanded and collapsed.
export function toggleExpandedKey(expandedKeys: Set<string>, key: string) {
  const next = new Set(expandedKeys);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}
