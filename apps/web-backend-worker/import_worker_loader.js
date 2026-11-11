import initializeWasm, {ImportHasher} from './cpu_worker.js';
import {startImportWorker} from './import_worker.js';
await initializeWasm();
startImportWorker(ImportHasher);
