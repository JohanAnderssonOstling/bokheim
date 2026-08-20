import { APP_NAME_LOWER } from "../../../../lib/app/application-name";

export const READER_UI_PREFERENCES_KEY = `${APP_NAME_LOWER}-reader-ui-preferences`;
export const READER_UI_PREFERENCES_VERSION = 2;

export function isKoboReader() {
  return /kobo/i.test(navigator.userAgent);
}

export function initialTocState() {
  try {
    const stored = localStorage.getItem(READER_UI_PREFERENCES_KEY);
    const parsed = stored ? JSON.parse(stored) : {};
    if (typeof parsed === "boolean") return isKoboReader() ? false : parsed;
    if (parsed && typeof parsed === "object" && typeof parsed.tocOpen === "boolean") {
      if (typeof parsed.version === "number") return parsed.tocOpen;
      return isKoboReader() ? false : parsed.tocOpen;
    }
  } catch {
    // Ignore malformed or inaccessible persisted state.
  }
  return false;
}

export function persistTocState(open: boolean) {
  try {
    localStorage.setItem(
      READER_UI_PREFERENCES_KEY,
      JSON.stringify({ version: READER_UI_PREFERENCES_VERSION, tocOpen: open }),
    );
  } catch {
    // Ignore persistence failures.
  }
}
