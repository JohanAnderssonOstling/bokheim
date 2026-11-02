// Dictionary components live as ordinary OPFS files, outside SQLite's VFS.
async function directory(id, create) {
    const root = await navigator.storage.getDirectory();
    const dictionaries = await root.getDirectoryHandle("bokheim-dictionaries-v1", { create });
    return dictionaries.getDirectoryHandle(id, { create });
}

export async function dictionaryOpen(id, name) {
    const folder = await directory(id, false);
    const file = await folder.getFileHandle(name);
    return file.createSyncAccessHandle();
}

export async function dictionaryWrite(id, name, bytes) {
    const folder = await directory(id, true);
    const file = await folder.getFileHandle(name, { create: true });
    const handle = await file.createSyncAccessHandle();
    try {
        handle.truncate(0);
        let offset = 0;
        while (offset < bytes.length) {
            const written = handle.write(bytes.subarray(offset), { at: offset });
            if (!written) throw new Error("Dictionary storage returned a short write");
            offset += written;
        }
        handle.flush();
    } finally { handle.close(); }
}

export async function dictionaryRemove(id) {
    try {
        const root = await navigator.storage.getDirectory();
        const dictionaries = await root.getDirectoryHandle("bokheim-dictionaries-v1");
        await dictionaries.removeEntry(id, { recursive: true });
    } catch (error) {
        if (error.name !== "NotFoundError") throw error;
    }
}

export function dictionaryRead(handle, bytes, offset) { return handle.read(bytes, { at: offset }); }
export function dictionarySize(handle) { return handle.getSize(); }
export function dictionaryClose(handle) { handle.close(); }
