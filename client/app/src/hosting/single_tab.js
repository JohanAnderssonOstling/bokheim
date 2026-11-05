// The browser releases this lock when the owning page closes or reloads.
export async function acquireTab() {
    let acquired, failed;
    const ready = new Promise((resolve, reject) => { acquired = resolve; failed = reject; });
    navigator.locks.request('bokheim-tab-active-v1', {ifAvailable: true}, lock => {
        if (!lock) throw new Error('Bokheim is already open in another tab. Close that tab before opening this one.');
        acquired();
        return new Promise(() => {});
    }).catch(failed);
    await ready;
}
