import { commands } from "./commands";
import { isWebApp } from "./platform";
import type { AccountStatus } from "../shared/types";

const SIGNED_OUT: AccountStatus = { signed_in: false, server_url: null, email: null };

/**
 * A browser has to ask the server; a native build reads its own store, so it
 * gives up far sooner before deciding to start signed out.
 */
const TIMEOUT_MS = isWebApp ? 10_000 : 2_000;

/**
 * Whether anyone is signed in, as far as the shell needs to know.
 *
 * The account page owns the full picture — storage, transfers, sign-in and
 * sign-out. The shell only needs the verdict, because startup, catalogue
 * reconciliation and foreground sync all branch on it.
 */
class AccountStatusStore {
  #status = $state<AccountStatus>({ ...SIGNED_OUT });
  #resolved = $state(false);

  get status(): AccountStatus {
    return this.#status;
  }

  get signedIn(): boolean {
    return this.#status.signed_in;
  }

  /** True once the check has finished, whatever its verdict. */
  get resolved(): boolean {
    return this.#resolved;
  }

  set(status: AccountStatus): void {
    this.#status = status;
  }

  /**
   * Never rejects and never blocks startup indefinitely: a check that stalls
   * resolves as signed out, and the account page reports and retries it.
   */
  async load(): Promise<void> {
    this.#resolved = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      this.#status = await Promise.race([
        commands.getAccountStatus(),
        new Promise<never>((_, reject) => {
          timeout = setTimeout(
            () => reject(new Error("Session check timed out.")),
            TIMEOUT_MS,
          );
        }),
      ]);
    } catch {
      // Leaving the previous verdict in place: a timed-out refresh is not
      // evidence of a sign-out.
    } finally {
      if (timeout) clearTimeout(timeout);
      this.#resolved = true;
    }
  }
}

export const accountStatus = new AccountStatusStore();
