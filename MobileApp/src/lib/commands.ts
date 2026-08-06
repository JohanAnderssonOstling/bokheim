import { commands as nativeCommands } from "./bindings";
import { isNativeApp } from "./platform";
import { webDemoCommands } from "./web-demo-commands";
import { webHttpCommands } from "./web-http-commands";

function activeCommands(): typeof nativeCommands {
  if (isNativeApp) return nativeCommands;
  if (import.meta.env.VITE_BOKHEIM_BACKEND !== "http") return webDemoCommands as typeof nativeCommands;
  return webHttpCommands;
}

export const commands = new Proxy({} as typeof nativeCommands, {
  get(_target, property) {
    const source = activeCommands();
    const value = source[property as keyof typeof nativeCommands];
    return typeof value === "function" ? value.bind(source) : value;
  },
});
