import {otherTabs, tryAcquireTab} from './single_tab.js';
import {text} from './ui-text.mjs';

export async function waitForTab(panel) {
    if (await tryAcquireTab()) return;
    const status = panel.querySelector('[data-startup-message]') || panel;
    const progress = panel.querySelector('progress');
    const message = document.createElement('p');
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = text('web.continue_here');
    button.disabled = true;
    status.replaceChildren(message, button);
    if (progress) progress.hidden = true;
    let finished = false;
    let checking = false;
    let opening = false;
    let lastCount;
    let lastActive;
    const update = async () => {
        if (finished || checking) return;
        checking = true;
        try {
            const tabs = await otherTabs();
            if (finished) return;
            if (tabs.count !== lastCount || tabs.active !== lastActive) {
                lastCount = tabs.count;
                lastActive = tabs.active;
                const count = tabs.count === 1 ? text('web.one_other_tab_open') : text('web.other_tabs_open', {count: tabs.count});
                let instruction = text('web.ready_to_open_here');
                if (tabs.active) {
                    instruction = tabs.count === 1 ? text('web.close_running_tab_to_continue') : text('web.close_other_tabs_to_continue');
                }
                message.textContent = `${count} ${instruction}`;
            }
            button.disabled = tabs.active || opening;
        } finally {
            checking = false;
        }
    };
    const refresh = () => { void update().catch(error => console.warn('Could not update open tab count', error)); };
    let timer;
    let onClick;
    try {
        await update();
        timer = setInterval(refresh, 500);
        document.addEventListener('visibilitychange', refresh);
        await new Promise((resolve, reject) => {
            onClick = async () => {
                if (opening) return;
                opening = true;
                button.disabled = true;
                try {
                    if (await tryAcquireTab()) { resolve(); return; }
                    opening = false;
                    await update();
                } catch (error) { reject(error); }
            };
            button.addEventListener('click', onClick);
        });
    } finally {
        finished = true;
        clearInterval(timer);
        document.removeEventListener('visibilitychange', refresh);
        if (onClick) button.removeEventListener('click', onClick);
    }
}
