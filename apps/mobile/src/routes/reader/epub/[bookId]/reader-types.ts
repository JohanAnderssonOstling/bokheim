import type { FoliateSearchMatch } from "../../../../lib/shared/types";

export interface TocItem {
  key: string;
  label: string;
  href?: string;
  depth: number;
  parentKeys: string[];
  disabled?: boolean;
  hasChildren: boolean;
}

export interface SearchResult extends FoliateSearchMatch {
  section: string;
}
