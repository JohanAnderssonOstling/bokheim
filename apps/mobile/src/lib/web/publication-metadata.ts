import { inspectEpubPublication, type PublicationMetadata } from "./wasm-core.ts";

function extension(fileName: string): string {
  return fileName.split(".").at(-1)?.toLocaleLowerCase() ?? "";
}

export async function inspectPublication(_file: File, fileName: string, bytes: Uint8Array): Promise<PublicationMetadata> {
  if (!bytes.length) throw new Error("The selected publication is empty");
  const kind = extension(fileName);
  if (kind === "epub") return inspectEpubPublication(bytes);
  if (kind === "pdf") {
    const signature = new TextDecoder("ascii").decode(bytes.subarray(0, 5));
    if (signature !== "%PDF-") throw new Error("The selected file is not a valid PDF document");
    return { title: null, author: null, description: null, subjects: [], subject_paths: [], isbns: [], cover: null, toc: [] };
  }
  throw new Error("The web library currently accepts EPUB and PDF files");
}
