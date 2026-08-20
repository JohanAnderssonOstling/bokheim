import type { commands as nativeCommands } from "../bindings";

/**
 * Stands in for the browser backend in native builds.
 *
 * Vite aliases `web-commands.ts` to this module when `BOKHEIM_BACKEND=native`,
 * which is what keeps `worker-client` — and through it the SQLite worker, the
 * TypeScript library database, and both WASM payloads — out of the APK. The
 * alias is also the enforcement: importing the browser backend from anywhere
 * else in a native build resolves here and fails on use, instead of quietly
 * adding megabytes of unreachable code.
 */
export const webCommands = new Proxy({} as typeof nativeCommands, {
  get(_target, property) {
    return () => {
      throw new Error(
        `The browser backend is not part of native builds (called "${String(property)}")`,
      );
    };
  },
});
