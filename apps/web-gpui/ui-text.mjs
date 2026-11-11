// Generated from apps/ui-text/english.json by export.py.
const messages = {
    "browser.reload": "Reload",
    "dictionary.disk_full": "Disk full.",
    "web.browser_too_old": "Update {name} to version {minimum} or newer to use Bokheim.",
    "web.close_other_app_tabs_then_reload": "Close other app tabs, then reload.",
    "web.close_other_tabs_to_continue": "Close the other tabs, then continue here.",
    "web.close_running_tab_to_continue": "Close the other tab, then continue here.",
    "web.continue_here": "Continue here",
    "web.couldn_t_start_the_app_reload_to_retry": "Couldn't start the app.",
    "web.downloading_app": "Downloading app",
    "web.downloading_app_progress": "Downloading app {percent}%",
    "web.loading_app": "Loading app",
    "web.loading_app_progress": "Loading app {percent}%",
    "web.missing_async_wait": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_exceptions": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_isolation": "Bokheim couldn’t start on this website. Please try again later.",
    "web.missing_locks": "This browser can’t save books. Update it or try another browser.",
    "web.missing_private_storage": "This browser can’t save books. Update it or try another browser.",
    "web.missing_shared_memory": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_workers": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.one_other_tab_open": "1 other Bokheim tab is open.",
    "web.opening_bokheim": "Opening Bokheim",
    "web.other_tabs_open": "{count} other Bokheim tabs are open.",
    "web.ready_to_open_here": "Bokheim can now open in this tab.",
    "web.requires_browser": "Bokheim requires {name} {minimum} or newer.",
    "web.retrying": "Retrying",
    "web.running_in_another_tab": "Bokheim is running in another tab.",
    "web.starting_app": "Starting app",
    "web.update_your_browser_to_its_latest_version": "Update your browser to its latest version.",
    "web.use_https": "Open https://app.bokheim.se to use Bokheim."
};

export function text(key, parameters = {}) {
    const message = messages[key];
    if (message === undefined) throw new Error(`Unknown UI text key: ${key}`);
    return message.replace(/\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g, (_, name) => {
        if (!(name in parameters)) throw new Error(`Missing UI text parameter: ${key}.${name}`);
        return String(parameters[name]);
    });
}
