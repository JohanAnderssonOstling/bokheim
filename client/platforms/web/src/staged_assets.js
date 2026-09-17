// Asset files outside SQLite's handle pool. Rust owns book and lifetimes.
const handles = new Map();
const opening = new Map();
async function stagedFile(name, create = false) {
    if (!/^(__import_objects\/[0-9a-f-]{36}\/\d+|__(book|thumbnail)_objects\/[0-9a-f-]+)$/.test(name)) throw Error('Invalid staged file path');
    const parts = name.split('/');
    let directory = await navigator.storage.getDirectory();
    for (const component of parts.slice(0, -1)) directory = await directory.getDirectoryHandle(component, {create});
    return directory.getFileHandle(parts.at(-1), {create});
}
export async function openStaged(name) {
    if (handles.has(name)) return handles.get(name).getSize();
    if (!opening.has(name)) opening.set(name, (async () => {
        const handle = await (await stagedFile(name)).createSyncAccessHandle();
        handles.set(name, handle);
        return handle.getSize();
    })().finally(() => opening.delete(name)));
    return opening.get(name);
}
export function closeStaged(name) {
    handles.get(name)?.close();
    handles.delete(name);
}
export function readStaged(name, offset, bytes) { return handles.get(name).read(bytes, {at: offset}); }
export function retireStaged(name) {
    closeStaged(name);
    // Deletion follows index book; failure can be retried on cleanup.
    (async () => {
        let directory = await navigator.storage.getDirectory();
        for (const component of name.split('/').slice(0, -1)) directory = await directory.getDirectoryHandle(component);
        await directory.removeEntry(name.split('/').at(-1));
    })().catch(error => { if (error.name !== 'NotFoundError') console.warn('Could not remove retired asset file', error); });
}

export async function createAsset(name) {
    const handle = await (await stagedFile(name, true)).createSyncAccessHandle();
    handles.set(name, handle);
    handle.truncate(0);
}
export function writeStaged(name, offset, bytes) {
    const count = handles.get(name).write(bytes, {at: offset});
    if (count !== bytes.length) throw Error('Incomplete asset write');
}
export function flushStaged(name) { handles.get(name).flush(); }

export async function listAssets() {
    const root = await navigator.storage.getDirectory();
    const names = [];
    for (const folder of ['__book_objects', '__thumbnail_objects']) {
        let directory;
        try { directory = await root.getDirectoryHandle(folder); }
        catch (error) { if (error.name === 'NotFoundError') continue; throw error; }
        for await (const name of directory.keys()) names.push(folder + '/' + name);
    }
    return names;
}
export async function removeAsset(name) {
    const [folder, file] = name.split('/');
    try {
        const directory = await (await navigator.storage.getDirectory()).getDirectoryHandle(folder);
        await directory.removeEntry(file);
    } catch (error) {
        // A reader retirement can remove the same orphan during enumeration.
        if (error.name !== 'NotFoundError') throw error;
    }
}

export async function stagedBlob(name) { return (await stagedFile(name)).getFile(); }
