import {text} from './ui-text.mjs';

// Version floors are a compatibility policy, not a guarantee that a device can
// run the app. Probe the required capabilities before loading the WASM bundle.
export function browserVersion(userAgent) {
    let match;
    // iOS browser brands normally describe a WebKit shell, not desktop Chrome
    // or Firefox. Use the OS version for these shells.
    if (/iPhone|iPad|iPod/.test(userAgent) && (match = userAgent.match(/OS (\d+)[._](\d+)/))) {
        return {name: 'iOS browser', version: `${match[1]}.${match[2]}`, minimum: '16.4'};
    }
    if ((match = userAgent.match(/Firefox\/(\d+)/))) {
        return {name: 'Firefox', version: match[1], minimum: '114'};
    }
    if ((match = userAgent.match(/(?:Chrome|Chromium)\/(\d+)/))) {
        const android = /Android/.test(userAgent);
        return {name: android ? 'Chrome for Android' : 'Chrome / Chromium', version: match[1], minimum: android ? '148' : '109'};
    }
    if (/Safari\//.test(userAgent) && (match = userAgent.match(/Version\/(\d+(?:\.\d+)?)/))) {
        return {name: 'Safari', version: match[1], minimum: '16.4'};
    }
    return null;
}

function belowMinimum(browser) {
    const [major, minor = 0] = browser.version.split('.').map(Number);
    const [requiredMajor, requiredMinor = 0] = browser.minimum.split('.').map(Number);
    return major < requiredMajor || (major === requiredMajor && minor < requiredMinor);
}

export function compatibilityMessage(userAgent) {
    const browser = browserVersion(userAgent);
    if (browser && belowMinimum(browser)) {
        return text('web.requires_browser', browser);
    }
    return text('web.update_your_browser_to_its_latest_version');
}

// Detect standardized exception handling (exnref), rather than the older
// try/catch proposal. Module bytes are from wasm-feature-detect's
// exceptions-final probe (Copyright 2023 Google Inc., Apache-2.0):
// https://www.apache.org/licenses/LICENSE-2.0
// https://github.com/GoogleChromeLabs/wasm-feature-detect/blob/main/src/detectors/exceptions-final/index.js
function supportsUnwinding(wasm) {
    if (typeof wasm?.validate !== 'function' || !wasm.JSTag) return false;
    try {
        return wasm.validate(new Uint8Array([
            0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0,
            10, 16, 1, 14, 0, 2, 105, 31, 64, 1, 3, 0, 0, 11, 0, 11, 26, 11,
        ]));
    } catch {
        return false;
    }
}

export function requiredCapabilities(env = globalThis) {
    const issues = [];
    const browser = browserVersion(env.navigator?.userAgent || '');
    if (browser) {
        if (belowMinimum(browser)) {
            issues.push(text('web.browser_too_old', browser));
        }
    }
    if (!env.isSecureContext) issues.push(text('web.use_https'));
    if (!env.crossOriginIsolated) issues.push(text('web.missing_isolation'));
    if (!env.WebAssembly || typeof env.SharedArrayBuffer !== 'function') issues.push(text('web.missing_shared_memory'));
    if (!supportsUnwinding(env.WebAssembly)) issues.push(text('web.missing_exceptions'));
    if (typeof env.Atomics?.waitAsync !== 'function') issues.push(text('web.missing_async_wait'));
    if (typeof env.Worker !== 'function') issues.push(text('web.missing_workers'));
    if (typeof env.SharedWorker !== 'function') issues.push(text('web.missing_shared_workers'));
    if (typeof env.navigator?.storage?.getDirectory !== 'function') issues.push(text('web.missing_private_storage'));
    if (typeof env.navigator?.locks?.request !== 'function') issues.push(text('web.missing_locks'));
    return issues;
}

// Confirm that a shared module worker can actually load under this site's CSP.
// Dedicated workers are created by tabs, so no nested-worker probe is needed.
export function checkSharedWorkerSupport() {
    return new Promise(resolve => {
        let worker;
        let url;
        let timer;
        const finish = issue => {
            clearTimeout(timer);
            if (worker) {
                worker.port.onmessage = null;
                worker.onerror = null;
                worker.port.close();
            }
            if (url) URL.revokeObjectURL(url);
            resolve(issue);
        };
        try {
            url = URL.createObjectURL(new Blob([
                'self.onconnect = ({ports:[port]}) => { port.postMessage(true); self.close(); };',
            ], {type: 'text/javascript'}));
            worker = new SharedWorker(url, {type: 'module'});
            timer = setTimeout(() => finish(text('web.worker_timeout')), 10000);
            worker.onerror = event => {
                event.preventDefault();
                finish(text('web.worker_load_failed'));
            };
            worker.port.onmessage = () => finish(null);
            worker.port.start();
        } catch {
            finish(text('web.workers_blocked'));
        }
    });
}

export async function checkCompatibility() {
    const issues = requiredCapabilities();
    if (issues.length) return issues;
    const workerIssue = await checkSharedWorkerSupport();
    return workerIssue ? [workerIssue] : [];
}
