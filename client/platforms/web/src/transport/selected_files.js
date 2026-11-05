// Files are browser objects. Clone them once to their storage owner; library
// operations and their results use Rust channels in shared Wasm memory.
const channelName = 'bokheim-selected-files-v1';
const selected = new Map();
let receiver;
export function initializeSelectedFiles() {
    receiver = new BroadcastChannel(channelName);
    receiver.onmessage = ({data}) => {
        if (data.kind === 'release') { selected.delete(data.token); return; }
        if (data.kind !== 'files') return;
        selected.set(data.token, data.files);
        receiver.postMessage({kind: 'received', token: data.token});
    };
}
export function takeSelectedFile(token, index) {
    const files = selected.get(token);
    if (!files) return undefined;
    const file = files[index];
    files[index] = undefined;
    return file;
}
export function releaseSelectedFiles(token) { selected.delete(token); }
export async function sendSelectedFiles(token, files, signal) {
    const channel = new BroadcastChannel(channelName);
    let aborted;
    try {
        await new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error('The storage worker did not receive the selected files')), 30000);
            aborted = () => {
                clearTimeout(timeout);
                channel.postMessage({kind: 'release', token});
                reject(new Error('Sending selected files was cancelled'));
            };
            signal.addEventListener('abort', aborted, {once: true});
            if (signal.aborted) { aborted(); return; }
            channel.onmessage = ({data}) => {
                if (data.kind === 'received' && data.token === token) { clearTimeout(timeout); resolve(); }
            };
            channel.onmessageerror = () => { clearTimeout(timeout); reject(new Error('Could not send selected files to storage')); };
            channel.postMessage({kind: 'files', token, files});
        });
    } finally { signal.removeEventListener('abort', aborted); channel.close(); }
}
