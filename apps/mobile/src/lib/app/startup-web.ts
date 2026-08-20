import { page } from "$app/state";

import { commands } from "./commands";
import { isReaderRoute } from "./app-utils";
import { accountStatus } from "./account-status.svelte";
import { librarySession } from "./library-session.svelte";
import { preferences } from "./preferences.svelte";
import { shell } from "./shell-state.svelte";
import { errorMessage } from "./startup";
import { clearThumbnailCache } from "../library/thumbnail-cache";

/**
 * Web startup.
 *
 * Unlike native, the account has to resolve first: a signed-in browser session
 * gets its libraries from the server, so loading them before the session is
 * known would show an empty catalogue and then replace it.
 */
export async function startWebApplication(): Promise<void> {
  shell.setWebReady(false);
  const session = librarySession.read();
  try {
    await accountStatus.load();
    await Promise.all([
      librarySession.load(),
      preferences.loadBrowsing(),
      preferences.loadReader(),
    ]);
    await librarySession.restore(session);
  } finally {
    shell.setRoutesReady();
    shell.setWebReady(true);
  }
  if (accountStatus.signedIn)
    void enrolRemoteLibraries().catch((reason) => shell.setError(errorMessage(reason)));
}

/**
 * Pulls libraries the account owns but this browser has never seen. Runs after
 * startup so a returning user sees their local state immediately.
 */
async function enrolRemoteLibraries(): Promise<void> {
  const [enrollment] = await Promise.allSettled([commands.bootstrapRemoteLibraries()]);
  if (enrollment.status === "rejected") shell.setError(errorMessage(enrollment.reason));
  clearThumbnailCache();
  await librarySession.refresh(false);
}

const FOREGROUND_SYNC_INTERVAL_MS = 30_000;

let inFlight: Promise<void> | undefined;
let lastRunAt = 0;

/**
 * Keeps a returning tab honest.
 *
 * Sign-in state can change in another tab, so coming back to the foreground
 * re-reads the session and syncs. Native does not need this: it learns about
 * changes from backend events instead. The throttle keeps a user flicking
 * between tabs from re-syncing on every focus.
 */
export function synchronizeForegroundWebApp(force = false): Promise<void> {
  if (!shell.webReady || !accountStatus.resolved) return Promise.resolve();
  if (document.visibilityState === "hidden") return Promise.resolve();
  if (inFlight) return inFlight;
  if (!force && Date.now() - lastRunAt < FOREGROUND_SYNC_INTERVAL_MS)
    return Promise.resolve();
  lastRunAt = Date.now();
  inFlight = (async () => {
    accountStatus.set(await commands.getAccountStatus());
    if (!accountStatus.signedIn) return;
    await commands.syncAllLibraries();
    await librarySession.refresh(isReaderRoute(page.route.id));
  })()
    .catch(() => undefined)
    .finally(() => (inFlight = undefined));
  return inFlight;
}
