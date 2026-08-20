import type { BookCardData, LibraryDirectory } from "../shared/types";

export type FolderPickerOperation =
  | { kind: "copy_book"; book: BookCardData; sourceId: string }
  | { kind: "move_book"; book: BookCardData; sourceId: string }
  | { kind: "move_folder"; folder: LibraryDirectory; sourceParentId: string };
