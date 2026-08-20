import { commands } from "../app/commands";

/**
 * Platform-neutral folder operations. UI components and controllers use this
 * boundary rather than knowing whether the application is backed by Tauri or
 * the web worker.
 */
export const folders = {
  create(libraryId: string, parentDirectoryId: string, name: string) {
    return commands.createDirectory(libraryId, parentDirectoryId, name);
  },
};
