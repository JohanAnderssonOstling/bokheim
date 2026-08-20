/**
 * Flattens foliate's nested table of contents into rows carrying their depth
 * and their ancestry.
 *
 * The list is stored flat and rendered flat — depth is an indent, not nesting —
 * so whether an entry is showing is a lookup against its ancestors rather than
 * a tree walk. Keys come from position, because hrefs repeat and some entries
 * have none.
 */
export function flatten(items, depth = 0, parentKeys = [], prefix = "") {
  let rows = [];
  (items ?? []).forEach(function (item, index) {
    let key = `${prefix}${index}`;
    let children = item.subitems ?? [];
    rows.push({
      key,
      label: item.label,
      href: item.href,
      depth,
      parentKeys,
      hasChildren: children.length > 0,
    });
    rows = rows.concat(flatten(children, depth + 1, [...parentKeys, key], `${key}.`));
  });
  return rows;
}
