export type ScreenPhase = "browser" | "opening-reader" | "reader" | "closing-reader";

export type ScreenTransitionState<T> =
  | Readonly<{ phase: "browser" | "opening-reader" | "closing-reader"; reader: null }>
  | Readonly<{ phase: "reader"; reader: T }>;

export type ScreenTransitionToken = Readonly<{ generation: number }>;

type ReaderHandoff<T> = {
  token: ScreenTransitionToken;
  reader: T;
};

/**
 * Owns the exclusive browser/reader surface transition.
 *
 * Route loading has its own cancellation generation in App.svelte. This
 * controller is narrower: it guarantees that only one application surface is
 * interactive and that stale reader opens cannot publish after a close or a
 * newer route transition.
 */
export class ScreenTransitionController<T> {
  #state: ScreenTransitionState<T> = { phase: "browser", reader: null };
  #generation = 0;
  #returnPath = "";
  #readerKey = "";
  #restoringKey = "";
  #handoffs = new Map<string, ReaderHandoff<T>>();
  #publish: (state: ScreenTransitionState<T>) => void;

  constructor(publish: (state: ScreenTransitionState<T>) => void = () => undefined) {
    this.#publish = publish;
  }

  get state(): ScreenTransitionState<T> {
    return this.#state;
  }

  beginReaderOpen(returnPath: string, readerKey: string, reader: T): ScreenTransitionToken {
    const token = this.#nextGeneration();
    this.#returnPath = returnPath;
    this.#readerKey = "";
    this.#restoringKey = readerKey;
    this.#handoffs.clear();
    this.#handoffs.set(readerKey, { token, reader });
    this.#setState({ phase: "opening-reader", reader: null });
    return token;
  }

  beginReaderRestore(readerKey: string): ScreenTransitionToken | null {
    if ((this.#state.phase === "reader" && this.#readerKey === readerKey) || this.#restoringKey === readerKey) return null;
    const token = this.#nextGeneration();
    this.#readerKey = "";
    this.#restoringKey = readerKey;
    this.#setState({ phase: "opening-reader", reader: null });
    return token;
  }

  handoff(readerKey: string): ReaderHandoff<T> | null {
    return this.#handoffs.get(readerKey) ?? null;
  }

  releaseHandoff(readerKey: string, token: ScreenTransitionToken): void {
    if (this.#handoffs.get(readerKey)?.token.generation === token.generation) this.#handoffs.delete(readerKey);
  }

  commitReader(token: ScreenTransitionToken, readerKey: string, reader: T): boolean {
    if (!this.isCurrent(token) || (this.#state.phase !== "opening-reader" && this.#state.phase !== "reader")) return false;
    this.#readerKey = readerKey;
    this.#restoringKey = "";
    this.#setState({ phase: "reader", reader });
    return true;
  }

  cancelReaderOpen(token: ScreenTransitionToken): boolean {
    if (!this.isCurrent(token)) return false;
    this.#clearReaderState();
    this.#returnPath = "";
    this.#setState({ phase: "browser", reader: null });
    return true;
  }

  beginReaderClose(fallbackPath: string): { token: ScreenTransitionToken; destination: string } {
    const destination = this.#returnPath || fallbackPath;
    const token = this.#nextGeneration();
    this.#clearReaderState();
    this.#setState({ phase: "closing-reader", reader: null });
    return { token, destination };
  }

  finishReaderClose(token: ScreenTransitionToken): boolean {
    if (!this.isCurrent(token)) return false;
    this.#returnPath = "";
    this.#setState({ phase: "browser", reader: null });
    return true;
  }

  showBrowser(): void {
    if (this.#state.phase === "browser" && this.#state.reader === null) return;
    this.#nextGeneration();
    this.#returnPath = "";
    this.#clearReaderState();
    this.#setState({ phase: "browser", reader: null });
  }

  isCurrent(token: ScreenTransitionToken): boolean {
    return token.generation === this.#generation;
  }

  #nextGeneration(): ScreenTransitionToken {
    return { generation: ++this.#generation };
  }

  #clearReaderState(): void {
    this.#readerKey = "";
    this.#restoringKey = "";
    this.#handoffs.clear();
  }

  #setState(state: ScreenTransitionState<T>): void {
    this.#state = state;
    this.#publish(this.#state);
  }
}
