import {text} from './ui-text.mjs';

const RETRY_KEY = 'bokheim-startup-retry-v1';

export async function startApplication(panel, initialize, {reload = () => location.reload(), storage, onFailure = () => {}} = {}) {
    // Accessing sessionStorage itself can throw when browser storage is blocked.
    // In that case retain the manual Reload action without an automatic loop.
    if (storage === undefined) {
        try { storage = globalThis.sessionStorage; } catch { storage = null; }
    }
    const status = panel.querySelector('[data-startup-message]');
    const progress = panel.querySelector('progress');
    status.textContent = text('web.opening_bokheim');
    const ready = new Promise((resolve, reject) => {
        globalThis.__BOKHEIM_STARTUP_READY__ = resolve;
        globalThis.__BOKHEIM_STARTUP_FAILED__ = detail => reject(new Error(detail));
    });
    // A backend failure can arrive before the WASM initialization promise settles.
    ready.catch(() => {});
    try {
        await Promise.all([initialize(), ready]);
        try { storage.removeItem(RETRY_KEY); } catch { /* Storage may be disabled. */ }
        panel.remove();
        return true;
    } catch (error) {
        onFailure();
        console.error('Bokheim startup failed', error);
        const busy = /BOKHEIM_STORAGE_BUSY|creating sync access handle/.test(String(error));
        let retry = false;
        if (!busy) {
            try {
                retry = storage.getItem(RETRY_KEY) !== '1';
                if (retry) storage.setItem(RETRY_KEY, '1');
            } catch { retry = false; }
        }
        if (retry) {
            status.textContent = text('web.retrying');
            reload();
            return false;
        }
        const message = document.createElement('p');
        message.textContent = busy ? text('web.close_other_app_tabs_then_reload') : text('web.couldn_t_start_the_app_reload_to_retry');
        const button = document.createElement('button');
        button.textContent = text('browser.reload');
        button.addEventListener('click', () => reload());
        progress.hidden = true;
        status.replaceChildren(message, button);
        return false;
    } finally {
        delete globalThis.__BOKHEIM_STARTUP_READY__;
        delete globalThis.__BOKHEIM_STARTUP_FAILED__;
    }
}
