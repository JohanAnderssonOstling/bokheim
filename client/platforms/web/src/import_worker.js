import {closeStaged, createAsset, flushStaged, writeStaged} from './staged_assets.js';

let workerUrl;
let importer;

export function initializeImportWorker(url) {
    if (workerUrl === url) return;
    importer?.fail(new Error('The import worker was reinitialized'));
    importer = undefined;
    workerUrl = url;
}

function aborted() { return new DOMException('Book processing was cancelled', 'AbortError'); }

// File objects and private staging names cross workers; book bytes stay local.
class ImportWorker {
    constructor(url) {
        this.url = url;
        this.queue = [];
        this.next = 1;
        this.worker = undefined;
        this.current = undefined;
        this.ready = false;
    }
    run(file, path, signal, checksum) {
        if (signal.aborted) return Promise.reject(aborted());
        return new Promise((resolve, reject) => {
            const job = {id: this.next++, file, path, signal, checksum, resolve, reject, cancelled: false};
            job.abort = () => {
                job.cancelled = true;
                if (this.current === job) {
                    // Keep the caller's generation pinned until the worker
                    // finishes its read, closes any handle and acknowledges.
                    try { this.worker.postMessage({cancel: job.id}); }
                    catch (error) { this.fail(error); }
                } else {
                    this.queue = this.queue.filter(queued => queued !== job);
                    this.finish(job, aborted());
                }
            };
            signal.addEventListener('abort', job.abort, {once: true});
            this.queue.push(job);
            this.pump();
        });
    }
    finish(job, error, result) {
        job.signal.removeEventListener('abort', job.abort);
        if (error) job.reject(error);
        else job.resolve(result);
    }
    fail(error) {
        clearTimeout(this.startup);
        this.worker?.terminate();
        this.worker = undefined;
        this.ready = false;
        const jobs = this.queue.splice(0);
        if (this.current) jobs.unshift(this.current);
        this.current = undefined;
        for (const job of jobs) this.finish(job, error);
    }
    pump() {
        if (this.current || !this.queue.length) return;
        if (!this.worker) {
            let worker;
            try { worker = new Worker(this.url, {type: 'module', name: 'bokheim-import'}); }
            catch (error) { this.fail(error); return; }
            this.worker = worker;
            this.startup = setTimeout(() => {
                if (this.worker === worker) this.fail(new DOMException('The import worker did not finish starting', 'TimeoutError'));
            }, 30000);
            worker.onerror = event => {
                event.preventDefault();
                if (this.worker === worker) this.fail(new Error(event.message || 'The import worker stopped'));
            };
            worker.onmessageerror = () => {
                if (this.worker === worker) this.fail(new Error('Could not receive the file processing result'));
            };
            worker.onmessage = ({data}) => {
                if (this.worker !== worker) return;
                if (data.ready === true) {
                    clearTimeout(this.startup);
                    this.ready = true;
                    this.pump();
                    return;
                }
                const job = this.current;
                if (!job || data.id !== job.id) { this.fail(new Error('Invalid import worker reply')); return; }
                this.current = undefined;
                const error = job.cancelled ? aborted() : data.error ? Object.assign(new DOMException(data.error, data.errorName || 'Error'), {failure: data.failure}) : undefined;
                this.finish(job, error, {checksum: data.checksum, size: data.size});
                this.pump();
            };
        }
        if (!this.ready) return;
        const job = this.queue.shift();
        this.current = job;
        try { this.worker.postMessage({id: job.id, file: job.file, path: job.path, checksum: job.checksum}); }
        catch (error) { this.fail(error); }
    }
}

function processSelectedFile(file, path, signal, checksum) {
    if (!workerUrl) return Promise.reject(new Error('The import worker is not initialized'));
    importer ??= new ImportWorker(workerUrl);
    return importer.run(file, path, signal, checksum);
}

export function copySelectedFile(file, path, signal, checksum) {
    return processSelectedFile(file, path, signal, checksum);
}

export function hashStoredFile(file, signal) {
    return processSelectedFile(file, undefined, signal);
}

async function processFile(file, path, checksum, Hasher, job) {
    const copying = path !== undefined;
    if (checksum !== undefined && (!copying || !/^[0-9a-f]{64}$/.test(checksum))) throw new Error('Invalid verified book checksum');
    const size = file.size;
    if (!Number.isSafeInteger(size) || size < 0 || (copying && size === 0)) throw new Error('The book is empty or has an unsupported size');
    let hasher;
    try {
        if (checksum === undefined) hasher = new Hasher();
        if (copying) await createAsset(path);
        for (let offset = 0; offset < size;) {
            if (job.cancelled) throw aborted();
            const end = Math.min(offset + 32 * 1024 * 1024, size);
            const bytes = new Uint8Array(await file.slice(offset, end).arrayBuffer());
            if (job.cancelled) throw aborted();
            if (bytes.length !== end - offset) throw new Error('The book ended unexpectedly');
            if (copying) writeStaged(path, offset, bytes);
            hasher?.update(bytes);
            offset = end;
        }
        if (job.cancelled) throw aborted();
        if (copying) flushStaged(path);
        return {checksum: checksum ?? hasher.finalize(), size};
    } finally {
        try { hasher?.free(); }
        finally { if (copying) closeStaged(path); }
    }
}

// The dedicated instance does not open the database or parser's reference VFS.
export function startImportWorker(Hasher) {
    let current;
    self.onmessage = ({data}) => {
        if (data.cancel !== undefined) {
            if (current?.id === data.cancel) current.cancelled = true;
            return;
        }
        if (current) {
            self.postMessage({id: data.id, error: 'The import worker is already processing a file'});
            return;
        }
        const job = {id: data.id, cancelled: false};
        current = job;
        (async () => {
            try {
                const result = await processFile(data.file, data.path, data.checksum, Hasher, job);
                self.postMessage({id: job.id, ...result});
            } catch (error) {
                self.postMessage({id: job.id, error: error.message || String(error), errorName: error.name, failure: error.failure});
            } finally { current = undefined; }
        })();
    };
    self.postMessage({ready: true});
}
