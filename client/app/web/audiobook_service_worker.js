// Cache decoded WebAssembly responses for faster, offline startup.
self.addEventListener('install', event => event.waitUntil(self.skipWaiting()));
self.addEventListener('activate', event => event.waitUntil(self.clients.claim()));
self.addEventListener('fetch', event => {
    const url = new URL(event.request.url);
    if (event.request.method === 'GET' && url.origin === self.location.origin &&
        /^\/pkg\/[a-f0-9]{24}\/[^/]+\/[^/]+_bg\.wasm$/.test(url.pathname)) {
        event.respondWith(wasmResponse(event));
        return;
    }
});

// CacheStorage saves the fetch body's decoded bytes. Return its original
// Response through fetch so the browser can retain compiled WASM alongside it.
async function wasmResponse(event) {
    let cache;
    try {
        cache = await caches.open('bokheim-wasm-v1');
        const saved = await cache.match(event.request);
        if (saved) return saved;
    } catch {}
    const response = await fetch(event.request);
    if (cache && response.ok && response.body) {
        event.waitUntil(saveWasm(cache, event.request, response.clone()).catch(() => {}));
    }
    return response;
}

async function saveWasm(cache, request, response) {
    await cache.put(request, response);
    const current = new URL(request.url);
    const name = current.pathname.split('/').pop();
    const previous = (await cache.keys()).filter(request => {
        const stored = new URL(request.url);
        return stored.href !== current.href && stored.pathname.split('/').pop() === name;
    });
    await Promise.all(previous.map(request => cache.delete(request)));
}
