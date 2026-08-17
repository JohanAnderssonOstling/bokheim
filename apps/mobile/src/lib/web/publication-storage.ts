const ROOT_DIRECTORY = "bokheim-publications";

export interface StoredPublication {
  storageKey: string | null;
  fallbackBytes: Uint8Array | null;
}

function storageManager(): StorageManager | undefined {
  return (globalThis.navigator as Navigator | undefined)?.storage;
}

function unavailableStorageError(reason: unknown): boolean {
  return reason instanceof DOMException && [
    "SecurityError",
    "NotAllowedError",
    "NotSupportedError",
    "InvalidStateError",
  ].includes(reason.name);
}

async function publicationDirectory(libraryId: string, create: boolean): Promise<FileSystemDirectoryHandle | null> {
  const storage = storageManager();
  if (!storage?.getDirectory) return null;
  try {
    const root = await storage.getDirectory();
    const publications = await root.getDirectoryHandle(ROOT_DIRECTORY, { create });
    return await publications.getDirectoryHandle(libraryId, { create });
  } catch (reason) {
    if (!create && reason instanceof DOMException && reason.name === "NotFoundError") return null;
    // getDirectory() is exposed by some browsers even when the current origin
    // (for example a LAN HTTP address) is not allowed to use OPFS. In that case
    // callers use the SQLite BLOB fallback instead.
    if (unavailableStorageError(reason)) return null;
    throw reason;
  }
}

export function publicationStorageKey(libraryId: string, contentHash: string): string {
  return `${libraryId}/${contentHash}`;
}

function storageKeyParts(storageKey: string): [string, string] {
  const parts = storageKey.split("/");
  if (parts.length !== 2 || parts.some((part) => !/^[a-zA-Z0-9-]+$/.test(part))) throw new Error("The stored publication path is invalid");
  return [parts[0], parts[1]];
}

export async function storePublication(libraryId: string, contentHash: string, bytes: Uint8Array): Promise<StoredPublication> {
  const directory = await publicationDirectory(libraryId, true);
  if (!directory) return { storageKey: null, fallbackBytes: bytes };
  const file = await directory.getFileHandle(contentHash, { create: true });
  const writable = await file.createWritable();
  try {
    await writable.write(bytes.slice().buffer);
    await writable.close();
  } catch (reason) {
    await writable.abort().catch(() => undefined);
    throw reason;
  }
  return { storageKey: publicationStorageKey(libraryId, contentHash), fallbackBytes: null };
}

export async function readPublication(storageKey: string): Promise<Blob> {
  const [libraryId, contentHash] = storageKeyParts(storageKey);
  const directory = await publicationDirectory(libraryId, false);
  if (!directory) throw new Error("The browser publication storage is unavailable");
  return (await directory.getFileHandle(contentHash)).getFile();
}

export async function removePublication(storageKey: string): Promise<void> {
  const [libraryId, contentHash] = storageKeyParts(storageKey);
  const directory = await publicationDirectory(libraryId, false);
  if (!directory) return;
  try {
    await directory.removeEntry(contentHash);
  } catch (reason) {
    if ((!(reason instanceof DOMException) || reason.name !== "NotFoundError") && !unavailableStorageError(reason)) throw reason;
  }
}

export async function removeLibraryPublications(libraryId: string): Promise<void> {
  const storage = storageManager();
  if (!storage?.getDirectory) return;
  try {
    const root = await storage.getDirectory();
    const publications = await root.getDirectoryHandle(ROOT_DIRECTORY);
    await publications.removeEntry(libraryId, { recursive: true });
  } catch (reason) {
    if ((!(reason instanceof DOMException) || reason.name !== "NotFoundError") && !unavailableStorageError(reason)) throw reason;
  }
}
