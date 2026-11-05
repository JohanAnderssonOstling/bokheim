import initializeWasm, {startCpuWorker} from "./cpu_worker.js";
await initializeWasm();
await startCpuWorker();
