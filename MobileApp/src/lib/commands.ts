import { commands as nativeCommands } from "./bindings";
import { isNativeApp } from "./platform";
import { webHttpCommands } from "./web-http-commands";

function activeCommands(): typeof nativeCommands {
  if (isNativeApp) return nativeCommands;
  return webHttpCommands;
}

export const commands = new Proxy({} as typeof nativeCommands, {
  get(_target, property) {
    const source = activeCommands();
    const value = source[property as keyof typeof nativeCommands];
    return typeof value === "function" ? value.bind(source) : value;
  },
});
