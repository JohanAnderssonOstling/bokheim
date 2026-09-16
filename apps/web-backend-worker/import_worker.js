import {directoryIo, readFileBytes} from '../../client/platforms/web/import_io.js';
// Browser file capabilities. Rust owns import ordering, hashing and recovery.
export async function openImport(storage, job) {
    const root = await storage.getDirectory();
    const objects = await root.getDirectoryHandle('__import_objects', {create: true});
    const directory = await objects.getDirectoryHandle(job.id, {create: true});
    return directoryIo(directory, job.files);
}

export async function discoverImports(storage) {
    const root = await storage.getDirectory();
    let directory;
    try { directory = await root.getDirectoryHandle('__import_objects'); }
    catch (error) { if (error.name === 'NotFoundError') return []; throw error; }
    const entries = [];
    for await (const [id, folder] of directory.entries()) {
        if (folder.kind !== 'directory') continue;
        try {
            const read = name => readFileBytes(folder, name);
            entries.push({id, journal: await read('journal'), manifest: await read('manifest')});
        } catch (error) { console.warn('Could not read import', id, error); }
    }
    return entries;
}
