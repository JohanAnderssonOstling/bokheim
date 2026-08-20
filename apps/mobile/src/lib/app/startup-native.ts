import { accountStatus } from "./account-status.svelte";
import { librarySession } from "./library-session.svelte";
import { preferences } from "./preferences.svelte";
import { shell } from "./shell-state.svelte";
import { errorMessage } from "./startup";

/**
 * Native startup.
 *
 * The local database already holds the libraries, so they load immediately and
 * the account check runs alongside rather than gating them. Routes become ready
 * as soon as the restored session has been applied — a signed-out device is
 * fully usable, and the catalogue reconciliation that follows is an
 * enhancement, not a prerequisite.
 */
export async function startNativeApplication(): Promise<void> {
  try {
    const session = librarySession.read();
    const resolvingAccount = accountStatus.load();
    await Promise.all([
      librarySession.load(),
      preferences.loadBrowsing(),
      preferences.loadReader(),
    ]);
    await librarySession.restore(session);
    shell.setRoutesReady();
    await resolvingAccount;
    if (accountStatus.signedIn)
      void librarySession
        .reconcile()
        .catch((reason) => shell.setError(errorMessage(reason)));
  } finally {
    shell.setRoutesReady();
  }
}
