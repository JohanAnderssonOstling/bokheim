import initializeWasm, {startCpuWorker} from "./cpu_worker.js";
import {runWorkerEndpoint} from "./worker_endpoint.js";
async function initialize() { await initializeWasm(); await startCpuWorker(); }
await runWorkerEndpoint(initialize);
