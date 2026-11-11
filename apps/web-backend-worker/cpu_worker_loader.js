import initializeWasm, {startCpuWorker} from "./cpu_worker.js";
try {
    await initializeWasm();
    await startCpuWorker();
} catch (error) {
    self.postMessage({transport: 'endpoint_failed', error: {message: error.message || String(error), name: error.name, failure: error.failure}});
}
