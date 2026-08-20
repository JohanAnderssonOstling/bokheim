import "../../../../vendor/foliate-js/view.js";
import type { FoliateView, ReaderBook } from "../../../../lib/shared/types";
import { normalizeFoliateTarget } from "../../../../lib/reader/foliate-target";

/// The foliate events the reader listens to, in one place so teardown matches setup.
export interface FoliateViewHandlers {
  relocate: (event: Event) => void;
  link: (event: Event) => void;
  externalLink: (event: Event) => void;
  load: (event: Event) => void;
  drawAnnotation: (event: Event) => void;
  showAnnotation: (event: Event) => void;
  createOverlay: (event: Event) => void;
}

const EVENT_NAMES: Array<[string, keyof FoliateViewHandlers]> = [
  ["relocate", "relocate"],
  ["link", "link"],
  ["external-link", "externalLink"],
  ["load", "load"],
  ["draw-annotation", "drawAnnotation"],
  ["show-annotation", "showAnnotation"],
  ["create-overlay", "createOverlay"],
];

/**
 * Creates the `<foliate-view>` element and wires the reader's handlers to it.
 *
 * Setup and teardown are derived from one list so a handler cannot be added
 * without also being removed. `dispose` closes the view, which is what releases
 * the section documents and their own listeners.
 */
export function mountFoliateView(host: HTMLElement, handlers: FoliateViewHandlers) {
  const element = document.createElement("foliate-view") as FoliateView;
  element.className = "foliate-view";
  for (const [name, key] of EVENT_NAMES) element.addEventListener(name, handlers[key]);
  host.append(element);

  function dispose() {
    for (const [name, key] of EVENT_NAMES)
      element.removeEventListener(name, handlers[key]);
    element.close();
    element.remove();
  }

  return { element, dispose };
}

export interface OpenPublicationOptions {
  file: ReaderBook["file"];
  initialCfi?: string | null;
  initialTarget?: string | null;
  /// Checked between awaits so a reader closed mid-open stops early.
  isDisposed: () => boolean;
  /// Runs once the book is parsed but before it is laid out.
  onBookReady: (view: FoliateView) => void | Promise<void>;
}

/**
 * Opens a publication into an already mounted view.
 *
 * Parsing, then `init`, then an optional jump: the order matters, because
 * `init` is what restores the saved location and a target jump has to come
 * after it to win.
 */
export async function openPublication(
  element: FoliateView,
  options: OpenPublicationOptions,
) {
  const { file, isDisposed } = options;
  if (typeof file !== "string" && !(file instanceof File))
    throw new Error("Unsupported EPUB reader source");
  await element.open(file);
  if (isDisposed()) return;

  await options.onBookReady(element);
  if (isDisposed()) return;

  await element.init({
    lastLocation: options.initialCfi || undefined,
    showTextStart: !options.initialCfi,
  });
  if (isDisposed()) return;

  if (options.initialTarget)
    await element.goTo(normalizeFoliateTarget(options.initialTarget));
}

/// Releases whatever backing the publication was read from.
export function releaseEpubSource(file: ReaderBook["file"]) {
  if (typeof file === "string" && file.startsWith("blob:")) URL.revokeObjectURL(file);
}
