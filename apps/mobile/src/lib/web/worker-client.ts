import type { WorkerCommandRequest, WorkerCommandResponse } from "./worker-protocol";

interface PendingCommand {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
}

let backendWorker: Worker | undefined;
let nextCommandId = 1;
const pendingCommands = new Map<number, PendingCommand>();

function rejectPendingCommands(reason: Error) {
  for (const pending of pendingCommands.values()) pending.reject(reason);
  pendingCommands.clear();
}

function getBackendWorker(): Worker {
  if (backendWorker) return backendWorker;

  backendWorker = new Worker(new URL("../../workers/backend.worker.ts", import.meta.url), { type: "module" });
  backendWorker.addEventListener("message", (event: MessageEvent<WorkerCommandResponse>) => {
    const response = event.data;
    if (response?.kind !== "response") return;
    const pending = pendingCommands.get(response.id);
    if (!pending) return;
    pendingCommands.delete(response.id);
    if (response.ok) pending.resolve(response.value);
    else pending.reject(new Error(response.error));
  });
  backendWorker.addEventListener("error", (event) => {
    rejectPendingCommands(new Error(event.message || "The web backend worker stopped unexpectedly"));
  });
  backendWorker.addEventListener("messageerror", () => {
    rejectPendingCommands(new Error("The web backend returned an unreadable response"));
  });
  return backendWorker;
}

export function callWebBackend<T>(command: string, args: unknown[]): Promise<T> {
  const id = nextCommandId++;
  const request: WorkerCommandRequest = { kind: "command", id, command, args };
  return new Promise<T>((resolve, reject) => {
    pendingCommands.set(id, { resolve: (value) => resolve(value as T), reject });
    try {
      getBackendWorker().postMessage(request);
    } catch (reason) {
      pendingCommands.delete(id);
      reject(reason instanceof Error ? reason : new Error(String(reason)));
    }
  });
}
