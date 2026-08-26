/// <reference lib="webworker" />

import initWebCore, {
  bokheim_catalog_acquire,
  bokheim_database_open,
  bokheim_execute_command,
  bokheim_library_import,
  bokheim_library_import_catalog,
  bokheim_library_publication_asset,
  bokheim_schedule_mutation_sync,
} from "../lib/web/generated/bokheim_web_core.js";
import { renderPdfFirstPageJpeg } from "../lib/web/pdf-thumbnail";
import type {
  WorkerCommandRequest,
  WorkerCommandResponse,
} from "../lib/web/worker-protocol";

const wasmUrl = new URL(
  "../lib/web/generated/bokheim_web_core_bg.wasm",
  import.meta.url,
);

type BookDestination = { library_id: string; directory_id: string };
type CatalogAcquisition = {
  file_name: string;
  media_type: string;
  bytes: Uint8Array;
};
type PublicationAsset = {
  media_type: string;
  publication_bytes: Uint8Array;
};

let ready: Promise<void> | undefined;

function openBackend(): Promise<void> {
  return (ready ??= (async () => {
    await initWebCore({ module_or_path: wasmUrl });
    await bokheim_database_open();
  })());
}

function destination(value: unknown): BookDestination {
  if (!value || typeof value !== "object") throw new Error("destination is invalid");
  const candidate = value as Partial<BookDestination>;
  if (
    typeof candidate.library_id !== "string" ||
    typeof candidate.directory_id !== "string"
  )
    throw new Error("destination is invalid");
  return candidate as BookDestination;
}

function bytes(value: unknown, name: string): Uint8Array {
  if (!(value instanceof Uint8Array)) throw new Error(`${name} must contain bytes`);
  return value;
}

async function importBrowserBook(args: unknown[]): Promise<null> {
  const target = destination(args[0]);
  const file = args[1];
  if (!(file instanceof File)) throw new Error("file is invalid");
  const fileName = args[2];
  if (typeof fileName !== "string") throw new Error("file_name must be a string");
  const cover = args[3] == null ? new Uint8Array() : bytes(args[3], "cover");
  bokheim_library_import(
    target.library_id,
    target.directory_id,
    fileName,
    new Uint8Array(await file.arrayBuffer()),
    cover,
  );
  bokheim_schedule_mutation_sync();
  return null;
}

async function importCatalogBook(args: unknown[]): Promise<null> {
  const target = destination(args[0]);
  const request = args[1];
  if (!request || typeof request !== "object")
    throw new Error("catalog request is invalid");
  const acquisition = (await bokheim_catalog_acquire(request)) as CatalogAcquisition;
  const file = new File([acquisition.bytes], acquisition.file_name, {
    type: acquisition.media_type,
  });
  const cover =
    (await renderPdfFirstPageJpeg(file, acquisition.file_name)) ?? new Uint8Array();
  bokheim_library_import_catalog(
    target.library_id,
    target.directory_id,
    acquisition.file_name,
    acquisition.bytes,
    cover,
    request,
  );
  bokheim_schedule_mutation_sync();
  return null;
}

function openBook(args: unknown[]): Blob {
  const libraryId = args[0];
  const contentHash = args[1];
  if (typeof libraryId !== "string" || typeof contentHash !== "string")
    throw new Error("library and publication identifiers must be strings");
  const asset = bokheim_library_publication_asset(
    libraryId,
    contentHash,
  ) as PublicationAsset;
  if (!asset.publication_bytes.length)
    throw new Error("The publication asset is not stored locally");
  return new Blob([asset.publication_bytes.slice().buffer], { type: asset.media_type });
}

async function execute(request: WorkerCommandRequest): Promise<unknown> {
  await openBackend();
  if (request.command === "import_book") return importBrowserBook(request.args);
  if (request.command === "import_catalog_book") return importCatalogBook(request.args);
  if (request.command === "open_book_blob") return openBook(request.args);
  return bokheim_execute_command(request.command, JSON.stringify(request.args));
}

type WorkerEndpoint = {
  addEventListener(
    type: "message",
    listener: (event: MessageEvent<WorkerCommandRequest>) => void,
  ): void;
  postMessage(message: WorkerCommandResponse): void;
};

const endpoint = self as unknown as WorkerEndpoint;
endpoint.addEventListener("message", async (event) => {
  const request = event.data;
  if (request?.kind !== "command") return;
  let response: WorkerCommandResponse;
  try {
    response = { kind: "response", id: request.id, ok: true, value: await execute(request) };
  } catch (reason) {
    response = {
      kind: "response",
      id: request.id,
      ok: false,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
  endpoint.postMessage(response);
});

export {};
