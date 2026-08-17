import type { Dir, LibraryEntry } from "../bindings";
import { commands } from "../commands";
import { ROOT_DIRECTORY_ID } from "../directory-id";
import { stageBrowserImport } from "../epub-picker";

const MAX_ARCHIVE_ENTRIES = 20_000;
const MAX_EPUB_BYTES = 1024 * 1024 * 1024;
const MAX_TOTAL_EPUB_BYTES = 4 * 1024 * 1024 * 1024;

type ZipEntry = {
  filename: string;
  directory?: boolean;
  uncompressedSize?: number;
  getData: (writer: unknown) => Promise<unknown>;
};

export interface LibraryArchiveImportResult {
  library: LibraryEntry;
  imported: number;
  skipped: number;
  failures: string[];
}

export interface FolderArchiveImportResult {
  folder: Dir;
  imported: number;
  skipped: number;
  failures: string[];
}

type ArchiveContents = {
  entries: ZipEntry[];
  candidates: Array<{ entry: ZipEntry; parts: string[] }>;
  name: string;
  wrapper: boolean;
};

export function pickLibraryArchive(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".zip,application/zip,application/x-zip-compressed";
    input.addEventListener("change", () => resolve(input.files?.[0] ?? null), { once: true });
    input.addEventListener("cancel", () => resolve(null), { once: true });
    input.click();
  });
}

function safeParts(filename: string): string[] | null {
  const normalized = filename.replaceAll("\\", "/");
  if (normalized.startsWith("/") || /^[a-z]:\//i.test(normalized)) return null;
  const parts = normalized.split("/").filter((part) => part && part !== ".");
  if (!parts.length || parts.some((part) => part === ".." || part.includes("\0"))) return null;
  return parts;
}

function archiveName(fileName: string): string {
  return fileName.replace(/\.zip$/i, "").trim().slice(0, 255) || "Imported library";
}

async function openArchive<T>(file: File, importContents: (contents: ArchiveContents, BlobWriter: new (type?: string) => unknown) => Promise<T>): Promise<T> {
  const { BlobReader, BlobWriter, configure, ZipReader } = await import("../../vendor/foliate-js/vendor/zip.js");
  configure({ useWebWorkers: false });
  const archive = new ZipReader(new BlobReader(file));
  try {
    const entries = await archive.getEntries() as ZipEntry[];
    if (entries.length > MAX_ARCHIVE_ENTRIES) throw new Error(`The ZIP contains more than ${MAX_ARCHIVE_ENTRIES.toLocaleString()} entries`);

    const candidates = entries.flatMap((entry) => {
      if (entry.directory || !/\.(?:epub|pdf)$/i.test(entry.filename)) return [];
      const parts = safeParts(entry.filename);
      return parts ? [{ entry, parts }] : [];
    });
    if (!candidates.length) throw new Error("The ZIP does not contain any EPUB or PDF books");

    const declaredBytes = candidates.reduce((total, { entry }) => total + (entry.uncompressedSize ?? 0), 0);
    if (candidates.some(({ entry }) => (entry.uncompressedSize ?? 0) > MAX_EPUB_BYTES)) throw new Error("The ZIP contains an EPUB larger than 1 GB");
    if (declaredBytes > MAX_TOTAL_EPUB_BYTES) throw new Error("The ZIP contains more than 4 GB of EPUB data");

    const name = archiveName(file.name);
    const wrapper = candidates.every(({ parts }) => parts.length > 1 && parts[0].toLocaleLowerCase() === name.toLocaleLowerCase());
    return await importContents({ entries, candidates, name, wrapper }, BlobWriter);
  } finally {
    await archive.close().catch(() => undefined);
  }
}

async function importCandidates(
  libraryId: string,
  baseDirectoryId: string,
  contents: ArchiveContents,
  BlobWriter: new (type?: string) => unknown,
): Promise<Omit<FolderArchiveImportResult, "folder">> {
  const directories = new Map<string, string>([["", baseDirectoryId]]);
  const failures: string[] = [];
  let imported = 0;
  for (const { entry, parts: originalParts } of contents.candidates) {
      const parts = contents.wrapper ? originalParts.slice(1) : originalParts;
      const fileName = parts.at(-1);
      if (!fileName) continue;
      try {
        let parentId = baseDirectoryId;
        let key = "";
        for (const segment of parts.slice(0, -1)) {
          key = key ? `${key}/${segment}` : segment;
          const existing = directories.get(key);
          if (existing) {
            parentId = existing;
            continue;
          }
          const directory = await commands.createDirectory(libraryId, parentId, segment);
          directories.set(key, directory.id);
          parentId = directory.id;
        }
        const isPdf = /\.pdf$/i.test(fileName);
        const mime = isPdf ? "application/pdf" : "application/epub+zip";
        const blob = await entry.getData(new BlobWriter(mime));
        if (!(blob instanceof Blob)) throw new Error("could not extract the publication");
        if (blob.size > MAX_EPUB_BYTES) throw new Error("Publication is larger than 1 GB");
        const publication = new File([blob], fileName, { type: mime });
        await commands.importBook({ library_id: libraryId, directory_id: parentId }, stageBrowserImport(publication), fileName);
        imported += 1;
      } catch (reason) {
        failures.push(`${entry.filename}: ${reason instanceof Error ? reason.message : String(reason)}`);
      }
  }
  return { imported, skipped: contents.entries.length - contents.candidates.length, failures };
}

export function importLibraryArchive(file: File): Promise<LibraryArchiveImportResult> {
  return openArchive(file, async (contents, BlobWriter) => {
    const library = await commands.createLibrary(contents.name);
    return { library, ...await importCandidates(library.library_id, ROOT_DIRECTORY_ID, contents, BlobWriter) };
  });
}

async function availableFolderName(libraryId: string, parentId: string, requested: string): Promise<string> {
  const existing = new Set((await commands.getChildDirectories(libraryId, parentId)).map(({ name }) => name.toLocaleLowerCase()));
  if (!existing.has(requested.toLocaleLowerCase())) return requested;
  for (let suffix = 2; suffix < 10_000; suffix += 1) {
    const candidate = `${requested.slice(0, Math.max(1, 250 - String(suffix).length))} (${suffix})`;
    if (!existing.has(candidate.toLocaleLowerCase())) return candidate;
  }
  throw new Error("Could not choose an unused folder name");
}

export function importFolderArchive(file: File, libraryId: string, parentDirectoryId: string): Promise<FolderArchiveImportResult> {
  return openArchive(file, async (contents, BlobWriter) => {
    const name = await availableFolderName(libraryId, parentDirectoryId, contents.name);
    const folder = await commands.createDirectory(libraryId, parentDirectoryId, name);
    return { folder, ...await importCandidates(libraryId, folder.id, contents, BlobWriter) };
  });
}
