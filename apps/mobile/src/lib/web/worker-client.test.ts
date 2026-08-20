import assert from "node:assert/strict";
import test from "node:test";

class FakePort extends EventTarget {
  messages: unknown[] = [];
  closed = false;
  postMessage(message: unknown): void {
    this.messages.push(message);
  }
  start(): void {}
  close(): void {
    this.closed = true;
  }
}

class FakeSharedWorker extends EventTarget {
  static instances: FakeSharedWorker[] = [];
  port = new FakePort();
  constructor() {
    super();
    FakeSharedWorker.instances.push(this);
  }
}

class FakeWorker extends EventTarget {
  postMessage(): void {}
  terminate(): void {}
}

test("worker errors reject pending calls and the next call creates a fresh endpoint", async () => {
  const originalSharedWorker = globalThis.SharedWorker;
  const originalWorker = globalThis.Worker;
  Object.defineProperty(globalThis, "SharedWorker", {
    configurable: true,
    value: FakeSharedWorker,
  });
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  try {
    const { callWebBackend } = await import("./worker-client.ts");
    const firstCall = callWebBackend("first", []);
    assert.equal(FakeSharedWorker.instances.length, 1);
    FakeSharedWorker.instances[0].dispatchEvent(
      new ErrorEvent("error", { message: "worker failed" }),
    );
    await assert.rejects(firstCall, /worker failed/);
    assert.equal(FakeSharedWorker.instances[0].port.closed, true);

    const secondCall = callWebBackend<string>("second", []);
    assert.equal(FakeSharedWorker.instances.length, 2);
    const request = FakeSharedWorker.instances[1].port.messages[0] as { id: number };
    FakeSharedWorker.instances[1].port.dispatchEvent(
      new MessageEvent("message", {
        data: { kind: "response", id: request.id, ok: true, value: "ready" },
      }),
    );
    assert.equal(await secondCall, "ready");
  } finally {
    Object.defineProperty(globalThis, "SharedWorker", {
      configurable: true,
      value: originalSharedWorker,
    });
    Object.defineProperty(globalThis, "Worker", {
      configurable: true,
      value: originalWorker,
    });
  }
});
