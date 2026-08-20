import { commands } from "../app/commands";
import { isWebApp } from "../app/platform";

/**
 * Pushes a freshly imported book to the server.
 *
 * Importing is owned by the route that did it, but the upload afterwards is
 * not page state, so it lives here rather than being repeated per route.
 * Signed-out users simply keep the book locally.
 */
export async function synchronizeBookImport(): Promise<void> {
  const status = await commands.getAccountStatus().catch(() => null);
  if (!status?.signed_in) return;
  // An empty browser library is deliberately not created remotely. Its first
  // import makes it eligible before mutation and blob synchronization run.
  if (isWebApp) await commands.reconcileLibraries();
  await commands.syncAllLibraries();
}
