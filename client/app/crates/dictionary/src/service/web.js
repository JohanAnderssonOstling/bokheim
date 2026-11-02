export function observe(root, changed) {
    const channel = new BroadcastChannel(`bokheim-dictionaries:${root}`);
    channel.onmessage = () => changed();
    return channel;
}
export function publish(channel) { channel.postMessage(null); }
export function close(channel) { channel.close(); }
