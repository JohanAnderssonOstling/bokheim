// Native host installs this in the main document after loading. Each document
// gets a fresh nonce so child frames and old pages cannot request credentials.
(request) => {
    if (window !== window.top || location.origin !== request.origin) return null;
    const visible = (input, editable = true) => {
        if ((editable && (input.disabled || input.readOnly)) || !input.getClientRects().length) return false;
        for (let element = input; element; element = element.parentElement) {
            const style = getComputedStyle(element);
            if (style.visibility !== 'visible' || style.opacity === '0') return false;
        }
        return true;
    };
    const candidate = (form = document.activeElement?.form) => {
        const passwords = [...(form || document).querySelectorAll('input[type="password"]')]
            .filter(input => visible(input) && input.autocomplete !== 'new-password');
        const current = passwords.filter(input => input.autocomplete === 'current-password');
        const password = current.length === 1 ? current[0] : passwords.length === 1 ? passwords[0] : null;
        if (!password || (password.form && new URL(password.form.action || location.href, document.baseURI).origin !== request.origin)) return null;
        const fields = [...(password.form ? password.form.elements : document.querySelectorAll('input'))]
            .filter(input => input instanceof HTMLInputElement && visible(input, false));
        const username = fields.find(input => input.autocomplete === 'username') ||
            fields.find(input => input.type === 'email') ||
            fields.find(input => input.type === 'text' && /user|email|login/i.test(input.name + ' ' + input.id));
        return {username, password};
    };
    if (request.kind === 'install') {
        const post = window.ipc?.postMessage?.bind(window.ipc);
        if (!post) return null;
        window.__bokheimPasswords?.stop();
        const controller = new AbortController();
        const touched = new WeakSet();
        const accounts = new WeakMap();
        let timer, verificationTimer, lastPassword, lastUsername, sequence = 0;
        let attempt = request.attempt, deadline = performance.now() + 60000, stableSince;
        const send = data => post(JSON.stringify({nonce: request.nonce, origin: request.origin, ...data}));
        const verify = () => {
            clearTimeout(verificationTimer);
            if (!attempt) return;
            if (performance.now() >= deadline) { attempt = null; return; }
            const formPresent = [...document.querySelectorAll('input[type="password"]')]
                .some(element => visible(element, false));
            // A brief hide/show during rendering does not count as disappearance.
            if (!formPresent) {
                stableSince ??= performance.now();
                if (performance.now() - stableSince >= 500) {
                    send({kind: 'form_gone', attempt});
                    attempt = null;
                    return;
                }
            } else { stableSince = null; }
            verificationTimer = setTimeout(verify, 250);
        };
        const schedule = () => {
            clearTimeout(timer);
            timer = setTimeout(() => {
                const fields = candidate();
                if (!fields || fields.password.value || touched.has(fields.password)) return;
                const username = fields.username?.value || '';
                if (lastPassword === fields.password && lastUsername === username) return;
                lastPassword = fields.password;
                lastUsername = username;
                send({kind: 'fill', username});
            }, 60);
        };
        window.addEventListener('submit', event => {
            if (!event.isTrusted || !navigator.userActivation.isActive || !(event.target instanceof HTMLFormElement)) return;
            const fields = candidate(event.target);
            if (!fields || !fields.password.value || fields.password.value.length > 2560) return;
            attempt = request.nonce + ':' + ++sequence;
            stableSince = null;
            deadline = performance.now() + 60000;
            send({kind: 'submitted', attempt, username: fields.username?.value || accounts.get(fields.password) || '', password: fields.password.value});
            verificationTimer = setTimeout(verify, 250);
        }, {capture: true, signal: controller.signal});
        document.addEventListener('focusin', () => { lastPassword = null; schedule(); }, {signal: controller.signal});
        document.addEventListener('input', event => {
            if (!event.isTrusted) return;
            if (event.target instanceof HTMLInputElement && event.target.type === 'password') touched.add(event.target);
            lastPassword = null;
            schedule();
        }, {signal: controller.signal});
        const observer = new MutationObserver(schedule);
        observer.observe(document.documentElement, {childList: true, subtree: true, attributes: true, attributeFilter: ['style', 'class', 'hidden', 'type', 'autocomplete']});
        window.__bokheimPasswords = {nonce: request.nonce, touched, accounts, stop: () => { clearTimeout(timer); clearTimeout(verificationTimer); controller.abort(); observer.disconnect(); }};
        schedule();
        verify();
        return true;
    }
    if (window.__bokheimPasswords?.nonce !== request.nonce) return null;
    const fields = candidate();
    if (!fields || fields.password.value || window.__bokheimPasswords.touched.has(fields.password) ||
        (fields.username?.value && fields.username.value !== request.username)) return null;
    if (fields.username && !fields.username.value && (fields.username.disabled || fields.username.readOnly)) return null;
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    const fill = (input, value) => {
        set.call(input, value);
        input.dispatchEvent(new Event('input', {bubbles: true}));
        input.dispatchEvent(new Event('change', {bubbles: true}));
    };
    if (fields.username && !fields.username.value) fill(fields.username, request.username);
    // Input handlers can navigate or replace the form while filling its username.
    if (location.origin !== request.origin || window.__bokheimPasswords?.nonce !== request.nonce ||
        !fields.password.isConnected || !visible(fields.password) || fields.password.value || (fields.username && fields.username.value !== request.username) ||
        (fields.password.form && new URL(fields.password.form.action || location.href, document.baseURI).origin !== request.origin)) return null;
    window.__bokheimPasswords.accounts.set(fields.password, request.username);
    fill(fields.password, request.password);
    return true;
}
