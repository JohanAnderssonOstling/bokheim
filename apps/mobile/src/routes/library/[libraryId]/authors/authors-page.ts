import type { AuthorSummary, DownloadState } from "../../../../lib/bindings";
import type { BookCardData } from "../../../../lib/shared/types";

/** Props shared by the authors index and its selected-author view. */
export interface AuthorsPageProps {
  libraryId: string;
  authors: AuthorSummary[];
  filter: string;
  focusedIndex: number;
  selectedAuthorId: string;
  selectedAuthorName: string;
  books: BookCardData[];
  detailed: boolean;
  openingBookId: string;
  downloadStates: Record<string, DownloadState>;
  onfilterkeydown: (event: KeyboardEvent) => void;
  onauthorkeydown: (event: KeyboardEvent, index: number) => void;
  onselectauthor: (id: string, name: string) => void | Promise<void>;
  onclear: () => void;
  onadd: () => void | Promise<void>;
  onopenbook: (contentHash: string) => void | Promise<void>;
  onopen: (book: BookCardData) => void | Promise<void>;
  ondownload: (book: BookCardData) => void | Promise<void>;
  onremove: (book: BookCardData) => void | Promise<void>;
  ontrash: (book: BookCardData) => void | Promise<void>;
}

export interface AuthorRowProps {
  author: AuthorSummary;
  libraryId: string;
  active?: boolean;
  authorIndex: number;
  tabindex?: number;
  onfocus?: () => void;
  onkeydown?: (event: KeyboardEvent) => void;
  onselect: () => void;
  onopenbook: (contentHash: string) => void | Promise<void>;
}
