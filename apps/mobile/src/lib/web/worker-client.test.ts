import assert from "node:assert/strict";
import test from "node:test";

class FakeWorker extends EventTarget {
  static instances: FakeWorker[] = [];
  messages: unknown[] = [];
  terminated = false;
  constructor() {
    super();
    FakeWorker.instances.push(this);
  }
  postMessage(message: unknown): void {
    this.messages.push(message);
  }
  terminate(): void {
    this.terminated = true;
  }
}

test("worker errors reject pending calls and the next call creates a fresh endpoint", async () => {
  const originalWorker = globalThis.Worker;
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  try {
    const { callWebBackend } = await import("./worker-client.ts");
    const firstCall = callWebBackend("first", []);
    assert.equal(FakeWorker.instances.length, 1);
    FakeWorker.instances[0].dispatchEvent(
      new ErrorEvent("error", { message: "worker failed" }),
    );
    await assert.rejects(firstCall, /worker failed/);
    assert.equal(FakeWorker.instances[0].terminated, true);

    const secondCall = callWebBackend<string>("second", []);
    assert.equal(FakeWorker.instances.length, 2);
    const request = FakeWorker.instances[1].messages[0] as { id: number };
    FakeWorker.instances[1].dispatchEvent(
      new MessageEvent("message", {
        data: { kind: "response", id: request.id, ok: true, value: "ready" },
      }),
    );
    assert.equal(await secondCall, "ready");
  } finally {
    Object.defineProperty(globalThis, "Worker", {
      configurable: true,
      value: originalWorker,
    });
  }
});
