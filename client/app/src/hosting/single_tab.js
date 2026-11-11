const ACTIVE_LOCK = 'bokheim-tab-active-v1';
const PRESENCE_PREFIX = 'bokheim-tab-presence-v1:';

// The startup module and wasm-bindgen's copy share ownership on this page.
function tabState() {
    return globalThis.__BOKHEIM_TAB_STATE_V1__ ??= {
        presenceName: PRESENCE_PREFIX + crypto.randomUUID(),
        presence: null,
        ownership: null,
    };
}

// Keep locks for the page's lifetime. Do not release on pagehide while storage
// workers may still be running; the browser releases them on close or reload.
function holdLock(name, options = {}) {
    return new Promise((resolve, reject) => {
        navigator.locks.request(name, options, lock => {
            if (!lock) { resolve(false); return; }
            resolve(true);
            return new Promise(() => {});
        }).catch(reject);
    });
}

export function registerTab() {
    const state = tabState();
    return state.presence ??= holdLock(state.presenceName);
}

export async function tryAcquireTab() {
    await registerTab();
    const state = tabState();
    // Waiting pages never queue for ownership: refreshing the running page
    // must not hand its storage to a different tab.
    state.ownership ??= holdLock(ACTIVE_LOCK, {ifAvailable: true});
    try {
        const acquired = await state.ownership;
        if (!acquired) state.ownership = null;
        return acquired;
    } catch (error) {
        state.ownership = null;
        throw error;
    }
}

export async function acquireTab() {
    if (!await tryAcquireTab()) {
        throw new Error('Bokheim is already open in another tab. Close that tab before opening this one.');
    }
}

export async function otherTabs() {
    await registerTab();
    const state = tabState();
    const {held} = await navigator.locks.query();
    const current = held.find(lock => lock.name === state.presenceName)?.clientId;
    // Include an older running app that has not registered a presence lock.
    const others = held.filter(lock => lock.clientId !== current && (lock.name.startsWith(PRESENCE_PREFIX) || lock.name === ACTIVE_LOCK));
    return {
        count: new Set(others.map(lock => lock.clientId)).size,
        active: others.some(lock => lock.name === ACTIVE_LOCK),
    };
}
