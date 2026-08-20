import { tick } from "svelte";

const enabled = import.meta.env.DEV;

function report(phase: string, operation: string, started: number, detail = "") {
  if (!enabled) return;
  const duration = performance.now() - started;
  console.info(
    `[performance] ${phase}.${operation} ${duration.toFixed(2)}ms${detail ? ` ${detail}` : ""}`,
  );
}

export async function measureInvoke<T>(
  operation: string,
  invoke: () => Promise<T>,
): Promise<T> {
  const started = performance.now();
  try {
    return await invoke();
  } finally {
    report("ipc", operation, started);
  }
}

export async function commitAndMeasureRender(
  operation: string,
  commit: () => void,
  itemCount: number,
): Promise<void> {
  const started = performance.now();
  commit();
  await tick();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  report("render", operation, started, `items=${itemCount}`);
}
