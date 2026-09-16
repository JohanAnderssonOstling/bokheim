// Startup must tolerate cold worker loading beyond the old 15-second deadline.
import assert from 'node:assert/strict';
const {firefox} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await firefox.launch({headless:true,executablePath:process.env.FIREFOX_PATH,
    firefoxUserPrefs:{'dom.webgpu.enabled':true,'gfx.webrender.all':true}});
try {
    const page = await browser.newPage();
    const dialogs = [];
    page.on('dialog', async dialog => {dialogs.push(dialog.message());await dialog.dismiss();});
    let delayed = false;
    await page.route('**/web_backend_worker_bg.wasm', async route => {
        if (!delayed) {delayed=true;await new Promise(resolve=>setTimeout(resolve,20000));}
        await route.continue();
    });
    await page.goto(process.env.BOKHEIM_TEST_URL || 'http://127.0.0.1:4173');
    await page.waitForSelector('canvas',{timeout:90000});
    assert.ok(delayed,'test must delay the database worker');
    assert.deepEqual(dialogs,[],'slow startup must not produce a database error');
    await page.reload();
    await page.waitForSelector('canvas',{timeout:60000});
    assert.deepEqual(dialogs,[],'reopening storage must succeed');
    console.log('PASS: database startup survives a 20-second worker download delay and reload');
} finally {await browser.close();}
