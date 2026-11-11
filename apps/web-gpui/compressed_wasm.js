let cacheReady;

// Cache installation must not delay the first download. Ordinary HTTP/WASM
// caching still works before the service worker takes control of the page.
function prepareCache() {
    if (!globalThis.document || !globalThis.navigator?.serviceWorker) return;
    cacheReady ??= navigator.serviceWorker.register(new URL('../../audiobook_service_worker.js', import.meta.url)).catch(() => {});
}

async function reportDownload(response, total) {
    const reader = response.clone().body.getReader();
    let loaded = 0;
    try {
        globalThis.__BOKHEIM_STARTUP_DOWNLOAD__?.({loaded, total});
        while (true) {
            const {done, value} = await reader.read();
            if (done) break;
            loaded += value.byteLength;
            globalThis.__BOKHEIM_STARTUP_DOWNLOAD__?.({loaded, total});
        }
        if (loaded !== total) throw new Error('The app download is incomplete. Reload to retry.');
        globalThis.__BOKHEIM_STARTUP_DOWNLOAD__?.({loaded, total, complete: true});
    } catch (error) {
        globalThis.__BOKHEIM_STARTUP_FAILED__?.(error);
    } finally {
        reader.releaseLock();
    }
}

export async function compressedWasm(url, totalBytes) {
    prepareCache();
    const response = await fetch(url, {cache: 'force-cache'});
    if (!response.ok) throw Object.assign(new Error(`App download failed (HTTP ${response.status}).`), {failure: response.status === 429 ? 'RateLimited' : response.status >= 500 ? 'ServiceUnavailable' : undefined});
    if (!response.body) throw new Error('The app download is empty. Reload to retry.');
    if (globalThis.document && globalThis.__BOKHEIM_STARTUP_DOWNLOAD__ && Number.isSafeInteger(totalBytes) && totalBytes > 0) {
        // Observe decoded payload bytes against their exact packaged size, not
        // Content-Length (which describes compressed bytes). Return the original
        // response to retain the browser's compiled-WASM cache association.
        void reportDownload(response, totalBytes);
    }
    return response;
}
