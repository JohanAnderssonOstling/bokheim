// Request the regular WASM URL. The server negotiates Brotli, gzip, or identity
// through Accept-Encoding; the browser decodes Content-Encoding before handing
// the streaming application/wasm response to wasm-bindgen.
export async function compressedWasm(url) {
    let response;
    try {
        response = await fetch(url, {cache: 'force-cache'});
    } catch {
        return null;
    }
    if (!response.ok || !response.body) return null;
    return response;
}
