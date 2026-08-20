import type { WorkerCommandRequest, WorkerCommandResponse } from "./worker-protocol.ts";
import { toCloneableWorkerValue } from "./worker-clone.ts";

interface PendingCommand {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

type BackendEndpoint = {
  postMessage(message: WorkerCommandRequest): void;
  addEventListener(
    type: "message",
    listener: (event: MessageEvent<WorkerCommandResponse>) => void,
  ): void;
  addEventListener(type: "messageerror", listener: (event: MessageEvent) => void): void;
  close?: () => void;
};

const COMMAND_TIMEOUT_MS = 5 * 60_000;

let backendWorker: Worker | SharedWorker | undefined;
let backendEndpoint: BackendEndpoint | undefined;
let nextCommandId = 1;
const pendingCommands = new Map<number, PendingCommand>();

function rejectPendingCommands(reason: Error) {
  for (const pending of pendingCommands.values()) {
    clearTimeout(pending.timeout);
    pending.reject(reason);
  }
  pendingCommands.clear();
}

function resetBackendWorker(reason: Error, endpoint: BackendEndpoint): void {
  if (backendEndpoint !== endpoint) return;
  rejectPendingCommands(reason);
  endpoint.close?.();
  if (backendWorker instanceof Worker) backendWorker.terminate();
  backendEndpoint = undefined;
  backendWorker = undefined;
}

function getBackendWorker(): BackendEndpoint {
  if (backendEndpoint) return backendEndpoint;

  if (typeof SharedWorker !== "undefined") {
    const sharedWorker = new SharedWorker(
      new URL("../../workers/backend.worker.ts", import.meta.url),
      {
        name: "bokheim-web-backend-v1",
        type: "module",
      },
    );
    backendWorker = sharedWorker;
    backendEndpoint = sharedWorker.port as BackendEndpoint;
    sharedWorker.port.start();
  } else {
    const dedicatedWorker = new Worker(
      new URL("../../workers/backend.worker.ts", import.meta.url),
      { type: "module" },
    );
    backendWorker = dedicatedWorker;
    backendEndpoint = dedicatedWorker as BackendEndpoint;
  }

  const endpoint = backendEndpoint;
  const worker = backendWorker;
  endpoint.addEventListener("message", (event: MessageEvent<WorkerCommandResponse>) => {
    const response = event.data;
    if (response?.kind !== "response") return;
    const pending = pendingCommands.get(response.id);
    if (!pending) return;
    pendingCommands.delete(response.id);
    clearTimeout(pending.timeout);
    if (response.ok) pending.resolve(response.value);
    else pending.reject(new Error(response.error));
  });
  endpoint.addEventListener("messageerror", () => {
    resetBackendWorker(
      new Error("The web backend returned an unreadable response"),
      endpoint,
    );
  });
  worker.addEventListener("error", (event: Event) => {
    const message = event instanceof ErrorEvent ? event.message : "";
    resetBackendWorker(
      new Error(message || "The web backend worker stopped unexpectedly"),
      endpoint,
    );
  });
  return endpoint;
}

export function callWebBackend<T>(command: string, args: unknown[]): Promise<T> {
  const id = nextCommandId++;
  const request: WorkerCommandRequest = {
    kind: "command",
    id,
    command,
    args: toCloneableWorkerValue(args),
  };
  return new Promise<T>((resolve, reject) => {
    const timeout = setTimeout(() => {
      if (!pendingCommands.delete(id)) return;
      reject(
        new Error(
          `The web backend did not finish ${command} within ${COMMAND_TIMEOUT_MS / 1000} seconds`,
        ),
      );
    }, COMMAND_TIMEOUT_MS);
    pendingCommands.set(id, { resolve: (value) => resolve(value as T), reject, timeout });
    try {
      getBackendWorker().postMessage(request);
    } catch (reason) {
      pendingCommands.delete(id);
      clearTimeout(timeout);
      reject(reason instanceof Error ? reason : new Error(String(reason)));
    }
  });
}
