// Real browser regression coverage with a fake account API and clock.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
const {chromium, firefox} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const source = await readFile(new URL('./authentication.js', import.meta.url));
const server = createServer((request, response) => {
    response.setHeader('Content-Type', request.url === '/authentication.js' ? 'text/javascript' : 'text/html');
    response.end(request.url === '/authentication.js' ? source : '<button id="opener">Sign in</button>');
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
try {
    for (const engine of [chromium, firefox]) {
        const browser = await engine.launch({headless: true});
        try {
            const page = await browser.newPage();
            const errors = [];
            page.on('pageerror', error => errors.push(error.message));
            await page.clock.install();
            await page.goto(`http://127.0.0.1:${server.address().port}/`);
            await page.evaluate(async () => {
                const {openAuthentication} = await import('/authentication.js');
                window.calls = [];
                window.failure = null;
                window.openPanel = () => {
                    document.querySelector('#opener').focus();
                    window.panel = openAuthentication('{}', 12, async json => {
                        calls.push(JSON.parse(json));
                        if (failure) throw Error(failure);
                    }, 60);
                };
                openPanel();
            });
            const button = name => page.getByRole('button', {name, exact: true});
            const pin = page.getByLabel('Verification PIN');
            async function register(email, password) {
                await button('Create account').click();
                await page.getByLabel('Email', {exact: true}).fill(email);
                await page.getByLabel('Password', {exact: true}).fill(password);
                await button('Create account').click();
                await page.getByRole('heading', {name: 'Check your email'}).waitFor();
            }
            await register('reader@example.com', 'correct horse battery');
            assert.equal(await page.locator('input').count(), 1);
            assert.equal(await pin.getAttribute('autocomplete'), 'one-time-code');
            assert.equal(await pin.evaluate(element => element === document.activeElement), true);
            assert.match(await page.getByRole('status').textContent(), /reader@example.com/);
            assert.equal(await button('Resend PIN (60s)').isDisabled(), true);
            // Waiting during the cooldown must not submit any requests.
            const count = await page.evaluate(() => calls.length);
            await page.clock.runFor(59_000);
            assert.equal(await button('Resend PIN (1s)').isDisabled(), true);
            assert.equal(await page.evaluate(() => calls.length), count);
            await page.clock.runFor(1_000);
            // Resend must work with the PIN still empty and without credential fields.
            await page.evaluate(() => failure = 'Could not send PIN');
            await button('Resend PIN').click();
            await page.getByRole('alert').filter({hasText: 'Could not send PIN'}).waitFor();
            assert.deepEqual(await page.evaluate(() => calls.at(-1)), {action: 'resend', email: 'reader@example.com'});
            await page.evaluate(() => failure = null);
            await button('Resend PIN').click();
            await page.getByRole('status').filter({hasText: 'A new PIN has been requested'}).waitFor();
            assert.equal(await button('Resend PIN (60s)').isDisabled(), true);
            await pin.fill('123456');
            await page.evaluate(() => failure = 'Invalid PIN');
            await button('Verify email').click();
            await page.getByRole('alert').filter({hasText: 'Invalid PIN'}).waitFor();
            assert.deepEqual(await page.evaluate(() => calls.at(-1)), {
                action: 'verify', email: 'reader@example.com', password: 'correct horse battery', pin: '123456',
            });
            assert.equal(await page.locator('input').count(), 1);
            await pin.fill('654321');
            await page.evaluate(() => failure = null);
            await button('Verify email').click();
            await page.locator('dialog').waitFor({state: 'detached'});
            assert.equal(await page.evaluate(() => calls.at(-1).pin), '654321');
            assert.equal(await page.evaluate(() => document.activeElement.id), 'opener');
            // A fresh signup must use its own credentials after restarting.
            await page.evaluate(() => openPanel());
            await register('first@example.com', 'first long password');
            await button('Use a different email').click();
            assert.equal(await page.getByLabel('Password', {exact: true}).inputValue(), '');
            await page.getByLabel('Email', {exact: true}).fill('second@example.com');
            await page.getByLabel('Password', {exact: true}).fill('second long password');
            await button('Create account').click();
            await pin.fill('123456');
            await button('Verify email').click();
            await page.locator('dialog').waitFor({state: 'detached'});
            assert.deepEqual(await page.evaluate(() => calls.at(-1)), {
                action: 'verify', email: 'second@example.com', password: 'second long password', pin: '123456',
            });
            // Leaving verification returns the carried email, with a fresh password field.
            await page.evaluate(() => openPanel());
            await register('third@example.com', 'third long password');
            await button('Back to sign in').click();
            assert.equal(await page.getByLabel('Email', {exact: true}).inputValue(), 'third@example.com');
            assert.equal(await page.getByLabel('Password', {exact: true}).inputValue(), '');
            await button('Close sign in').click();
            await page.clock.runFor(61_000);
            assert.deepEqual(errors, []);
            console.log(`PASS ${engine.name()}: PIN-only signup, retained credentials, resend cooldown, resend errors and retries, restart, and cleanup`);
        } finally { await browser.close(); }
    }
} finally { server.close(); }
