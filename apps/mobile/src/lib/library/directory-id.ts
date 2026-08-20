export const ROOT_DIRECTORY_ROUTE_ID = "__root__";
export const ROOT_DIRECTORY_ID = "00000000-0000-0000-0000-000000000000";

export function toBackendDirectoryId(directoryId: string): string {
  return directoryId === ROOT_DIRECTORY_ROUTE_ID ? ROOT_DIRECTORY_ID : directoryId;
}
