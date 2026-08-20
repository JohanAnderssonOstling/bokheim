/**
 * The little the application shell owns about itself.
 *
 * Startup reports progress here and the shell renders it. It is a module rather
 * than component state because the startup sequences drive it and they are not
 * components.
 */
class ShellState {
  /** Startup, library switching and reader opening report failures here. */
  #error = $state("");
  /** Routes may render, and the session may start being recorded. */
  #routesReady = $state(false);
  /** Web only: the application has rebuilt enough state to follow the URL. */
  #webReady = $state(false);

  get error(): string {
    return this.#error;
  }

  get routesReady(): boolean {
    return this.#routesReady;
  }

  get webReady(): boolean {
    return this.#webReady;
  }

  setError(message: string): void {
    this.#error = message;
  }

  clearError(): void {
    this.#error = "";
  }

  setRoutesReady(): void {
    this.#routesReady = true;
  }

  setWebReady(ready: boolean): void {
    this.#webReady = ready;
  }
}

export const shell = new ShellState();
