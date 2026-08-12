import { inspectEpubPublication, type PublicationMetadata } from "./wasm-core.ts";

function extension(fileName: string): string {
  return fileName.split(".").at(-1)?.toLocaleLowerCase() ?? "";
}

function validatePdf(bytes: Uint8Array): void {
  if (bytes.length < 5 || new TextDecoder("ascii").decode(bytes.subarray(0, 5)) !== "%PDF-") {
    throw new Error("The selected file is not a readable PDF");
  }
}

export async function inspectPublication(_file: File, fileName: string, bytes: Uint8Array): Promise<PublicationMetadata> {
  if (!bytes.length) throw new Error("The selected publication is empty");
  switch (extension(fileName)) {
    case "epub":
      return inspectEpubPublication(bytes);
    case "pdf":
      validatePdf(bytes);
      return { title: null, author: null, cover: null };
    default:
      return { title: null, author: null, cover: null };
  }
}
