let worker;
let failure;
let rejectWorker;

export function wake_pdf_worker(module, memory, failed, completed) {
    if (failure) throw failure;
    if (!worker) {
        const base = new URL(`${globalThis.__BOKHEIM_ASSET_BASE__ ?? './pkg'}/`, location.href);
        worker = new Worker(new URL('pdf_worker.js', base), { type: 'module', name: 'pdfium' });
        const current = worker;
        let retryable = true;
        const reject = error => {
            const message = error instanceof Error ? error : Object.assign(new Error(error?.message || String(error)), {name: error?.name || "Error", failure: error?.failure});
            if (worker !== current) return;
            failure = retryable ? undefined : message;
            current.terminate();
            worker = undefined;
            failed(message, retryable);
        };
        rejectWorker = reject;
        worker.onerror = event => { event.preventDefault(); reject(event.message || 'PDF worker crashed'); };
        worker.onmessageerror = () => reject('Invalid PDF worker response');
        worker.onmessage = event => {
            if (worker !== current) return;
            if (event.data?.initializing) retryable = false;
            else if (event.data?.error) reject(event.data.error);
            else if (event.data?.completed) completed(event.data.completed);
        };
        try {
            worker.postMessage({ module, memory });
        } catch (error) {
            reject(error);
            return;
        }
    }
    try {
        worker.postMessage({ run: true });
    } catch (error) {
        rejectWorker(error);
    }
}

// Called on the PDF worker after the result is published in shared memory.
export function complete_pdf_worker_job(id) {
    self.postMessage({ completed: id });
}
