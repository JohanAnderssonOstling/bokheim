/**
 * Startup sequences.
 *
 * Native and web load the same things in a different order, so each has its own
 * sequence rather than one with branches in it. Both reach their collaborators
 * — the account status, the library session, the preferences — directly; there
 * is one of each for the application, so handing them over bought nothing.
 */
export function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}
