import { redirect } from "@sveltejs/kit";

/**
 * The application has no root page of its own. `/library` owns picking which
 * library to open, so the entry point defers to it rather than resolving a
 * library here as well.
 */
export function load() {
  redirect(307, "/library");
}
