// Ordinary OPFS files, independently of SQLite's virtual filesystem.
async function parent(path, create) {
    const parts = path.split("/").filter(Boolean);
    if (!parts.length || parts.some(part => part === "." || part === "..")) {
        throw new Error("Invalid storage path");
    }
    const name = parts.pop();
    let directory = await navigator.storage.getDirectory();
    for (const part of parts) {
        directory = await directory.getDirectoryHandle(part, { create });
    }
    return { directory, name };
}

export async function storageReadAll(path) {
    try {
        const { directory, name } = await parent(path, false);
        const handle = await directory.getFileHandle(name);
        return new Uint8Array(await (await handle.getFile()).arrayBuffer());
    } catch (error) {
        if (error.name === "NotFoundError") return null;
        throw error;
    }
}

export async function storageWriteAtomic(path, bytes) {
    const { directory, name } = await parent(path, true);
    let file;
    let created = false;
    try {
        file = await directory.getFileHandle(name);
    } catch (error) {
        if (error.name !== "NotFoundError") throw error;
        file = await directory.getFileHandle(name, { create: true });
        created = true;
    }
    let writer;
    try {
        writer = await file.createWritable();
        await writer.write(bytes);
        await writer.close();
    } catch (error) {
        await writer?.abort().catch(() => {});
        if (created) await directory.removeEntry(name).catch(() => {});
        throw error;
    }
}

export async function storageOpen(path) {
    const { directory, name } = await parent(path, false);
    const file = await directory.getFileHandle(name);
    return file.getFile();
}

export async function storageRemoveDirectory(path) {
    try {
        const { directory, name } = await parent(path, false);
        await directory.removeEntry(name, { recursive: true });
    } catch (error) {
        if (error.name !== "NotFoundError") throw error;
    }
}

export async function storageFileSize(path) {
    try {
        return (await storageOpen(path)).size;
    } catch (error) {
        if (error.name === "NotFoundError") return null;
        throw error;
    }
}

export async function storageListDirectory(path) {
    try {
        const { directory, name } = await parent(path, false);
        const folder = await directory.getDirectoryHandle(name);
        const files = [];
        for await (const [name, handle] of folder.entries()) {
            if (handle.kind !== "file") continue;
            try {
                const file = await handle.getFile();
                files.push({ name, size: file.size, modified: file.lastModified });
            } catch (error) {
                if (error.name !== "NotFoundError") throw error;
            }
        }
        return files;
    } catch (error) {
        if (error.name === "NotFoundError") return [];
        throw error;
    }
}

export async function storageRemoveFile(path) {
    try {
        const { directory, name } = await parent(path, false);
        await directory.removeEntry(name);
    } catch (error) {
        if (error.name !== "NotFoundError") throw error;
    }
}

export async function storageRead(file, offset, length) {
    const bytes = new Uint8Array(await file.slice(offset, offset + length).arrayBuffer());
    if (bytes.length !== length) throw new Error("File ended before the requested range");
    return bytes;
}
export function storageSize(file) { return file.size; }

// A cancellable lock for read-modify-write operations across windows and workers.
export function storageBeginLock(path) {
    const controller = new AbortController();
    let release;
    let acquired, failed;
    const ready = new Promise((resolve, reject) => { acquired = resolve; failed = reject; });
    navigator.locks.request(`bokheim-files:${path}`, { signal: controller.signal }, () => new Promise(resolve => {
        release = resolve;
        acquired();
    })).catch(failed);
    return { ready, close() { controller.abort(); release?.(); } };
}
export function storageLockReady(ticket) { return ticket.ready; }
export function storageCloseLock(ticket) { ticket.close(); }
