import {
  listen as listenNative,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";
import { isNativeApp } from "./platform";

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  if (!isNativeApp) return Promise.resolve(() => undefined);
  return listenNative(event, handler);
}
