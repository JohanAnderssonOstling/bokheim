// Generated from apps/ui-text/english.json by export.py.
const messages = {
    "browser.reload": "Reload",
    "web.browser_too_old": "Update {name} to version {minimum} or newer to use Bokheim.",
    "web.close_other_app_tabs_then_reload": "Close other app tabs, then reload.",
    "web.couldn_t_start_the_app_reload_to_retry": "Couldn't start the app. Reload to retry.",
    "web.downloading_app": "Downloading app…",
    "web.downloading_app_progress": "Downloading app… {percent}%",
    "web.error_details": "Error details",
    "web.loading_app": "Loading app…",
    "web.loading_app_progress": "Loading app… {percent}%",
    "web.missing_async_wait": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_exceptions": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_isolation": "Bokheim couldn’t start on this website. Please try again later.",
    "web.missing_locks": "Bokheim can’t save books in this browser. Allow this site to save data, then reload.",
    "web.missing_private_storage": "Bokheim can’t save books in this browser. Allow this site to save data, then reload.",
    "web.missing_shared_memory": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.missing_workers": "Your browser can’t run Bokheim. Update it or try another browser.",
    "web.opening_bokheim": "Opening Bokheim…",
    "web.requires_browser": "Bokheim requires {name} {minimum} or newer.",
    "web.retrying": "Retrying…",
    "web.starting_app": "Starting app…",
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
