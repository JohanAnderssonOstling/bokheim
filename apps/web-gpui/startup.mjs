import {text} from './ui-text.mjs';
import {waitForTab} from './tabs.mjs';

export async function startApplication(panel, initialize, {reload = () => location.reload(), onFailure = () => {}} = {}) {
    // Tabs opened during deployment can still have the previous HTML shell.
    const status = panel.querySelector('[data-startup-message]') || panel;
    const progress = panel.querySelector('progress');
    let animation;
    let initializing = false;
    let started = false;
    let percent = -1;
    const beginInitialization = () => {
        if (initializing || started) return;
        initializing = true;
        status.textContent = text('web.starting_app');
        if (!progress) return;
        progress.max = 100;
        progress.value = 0;
        progress.setAttribute('aria-label', text('web.starting_app'));
        const since = performance.now();
        // Initialization has no measurable total. Estimate movement while
        // waiting for the real ready signal, and never complete on a timer.
        animation = setInterval(() => {
            progress.value = 95 * (1 - Math.exp(-(performance.now() - since) / 6000));
        }, 100);
    };
    status.textContent = text('web.loading_app');
    if (progress) {
        progress.max = 100;
        progress.value = 0;
        progress.setAttribute('aria-label', text('web.loading_app'));
    }
    let fail;
    const ready = new Promise((resolve, reject) => {
        fail = error => reject(error instanceof Error ? error : new Error(String(error)));
        globalThis.__BOKHEIM_STARTUP_READY__ = () => {
            started = true;
            if (progress) progress.value = 100;
            resolve();
        };
        globalThis.__BOKHEIM_STARTUP_FAILED__ = fail;
    });
    // A backend failure can arrive before the WASM initialization settles.
    ready.catch(() => {});
    globalThis.__BOKHEIM_STARTUP_DOWNLOAD__ = ({loaded, total, complete}) => {
        if (complete) {
            beginInitialization();
            return;
        }
        if (initializing || started) return;
        const next = Math.min(100, Math.floor(100 * loaded / total));
        if (next === percent) return;
        percent = next;
        status.textContent = text('web.loading_app_progress', {percent});
        if (progress) progress.value = next;
    };
    const failed = event => fail(event.error || event.reason || event.message || text('web.couldn_t_start_the_app_reload_to_retry'));
    globalThis.addEventListener('error', failed);
    globalThis.addEventListener('unhandledrejection', failed);
    try {
        await waitForTab(panel);
        status.textContent = text('web.loading_app');
        if (progress) progress.hidden = false;
        await Promise.all([Promise.resolve().then(initialize).then(beginInitialization), ready]);
        panel.remove();
        return true;
    } catch (error) {
        onFailure();
        console.error('Bokheim startup failed', error);
        const reason = error?.message || String(error);
        const busy = /BOKHEIM_STORAGE_BUSY|creating sync access handle/.test(reason);
        const message = document.createElement('p');
        message.textContent = busy ? text('web.close_other_app_tabs_then_reload') : text('web.couldn_t_start_the_app_reload_to_retry');
        const detail = document.createElement('details');
        const summary = document.createElement('summary');
        summary.textContent = text('web.error_details');
        const description = document.createElement('pre');
        description.textContent = reason;
        detail.append(summary, description);
        const button = document.createElement('button');
        button.textContent = text('browser.reload');
        button.addEventListener('click', () => reload());
        if (progress) progress.hidden = true;
        status.replaceChildren(message, detail, button);
        return false;
    } finally {
        clearInterval(animation);
        globalThis.removeEventListener('error', failed);
        globalThis.removeEventListener('unhandledrejection', failed);
        delete globalThis.__BOKHEIM_STARTUP_READY__;
        delete globalThis.__BOKHEIM_STARTUP_FAILED__;
        delete globalThis.__BOKHEIM_STARTUP_DOWNLOAD__;
    }
}
