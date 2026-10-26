#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! One browser verification attempt, or with `store` a store's window,
//! isolated from the GPUI event loop.
mod address;
mod passwords;
mod profile;
mod store;

use anyhow::{Result, bail};
use library_opds::browser::{Request, Response};
use serde::Deserialize;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use url::Url;
use wry::WebViewBuilder;

const ERROR: &str = "Browser verification could not be completed for this catalog. Please try another catalog.";
const READY_SCRIPT: &str = r#"
(() => {
    let verificationReported = false;
    window.addEventListener('message', event => {
        if (event.origin === 'https://challenges.cloudflare.com' &&
            event.data?.source === 'cloudflare-challenge' && event.data.event === 'fail') {
            window.ipc.postMessage(JSON.stringify({kind: 'verification_failed'}));
        }
    });
    const ready = () => {
        const challenged = (document.title || '').toLowerCase().includes('just a moment') ||
            Boolean(document.querySelector('#challenge-running, #challenge-stage, .cf-browser-verification'));
        if (challenged) {
            if (!verificationReported) {
                verificationReported = true;
                window.ipc.postMessage(JSON.stringify({kind: 'verification_required'}));
            }
            window.setTimeout(ready, 500);
            return;
        }
        window.ipc.postMessage(JSON.stringify({kind: 'ready'}));
    };
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready, {once: true});
    else ready();
})();
"#;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    Ready,
    VerificationRequired,
    VerificationFailed,
    Challenge,
    Fetched { status: u16, content_type: Option<String>, url: String, body: String },
    Error,
}

fn main() {
    // Checks runtime availability without creating a window: without WebKitGTK
    // this binary does not start, and without WebView2 there is no version.
    if std::env::args().nth(1).as_deref() == Some("probe") {
        std::process::exit(if wry::webview_version().is_ok() { 0 } else { 1 });
    }
    if std::env::args().nth(1).as_deref() == Some("store") {
        #[cfg(target_os = "linux")]
        // Wry's child window uses X11. This runs before creating any threads,
        // including when an older desktop executable launches the helper.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::set_var("GDK_BACKEND", "x11");
        }
        if let Err(error) = store::run() {
            eprintln!("Store window failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("OPDS browser verification failed: {error:#}");
        write_output(failure());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let input: Request = serde_json::from_str(&input)?;
    let target = Url::parse(&input.url)?;
    if !matches!(target.scheme(), "http" | "https") || !target.username().is_empty() || target.password().is_some() {
        bail!("verification requires an HTTP address without embedded credentials");
    }
    let origin = target.origin();
    let ipc_origin = origin.clone();
    let mut seed = target.clone();
    seed.set_path("/");
    seed.set_query(None);
    seed.set_fragment(None);
    if let Some(navigation_url) = &input.navigation_url {
        let navigation_url = Url::parse(navigation_url)?;
        if navigation_url.origin() != origin || !navigation_url.username().is_empty() || navigation_url.password().is_some() {
            bail!("the verification landing page must belong to the feed's origin and have no embedded credentials");
        }
        seed = navigation_url;
    }
    let script = fetch_script(&input)?;
    let event_loop = EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new().with_title("Bokheim – Feed verification").with_inner_size(LogicalSize::new(900.0, 700.0)).with_visible(false).build(&event_loop)?;
    let mut web_context = profile::context(&input.data_directory)?;
    let builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_url("about:blank")
        .with_initialization_script_for_main_only(READY_SCRIPT, true)
        .with_navigation_handler(move |url| url == "about:blank" || url == "about:srcdoc" || Url::parse(&url).is_ok_and(|url| url.origin() == origin || (url.scheme() == "https" && url.host_str() == Some("challenges.cloudflare.com"))))
        .with_ipc_handler(move |request| {
            if Url::parse(&request.uri().to_string()).is_ok_and(|url| url.origin() == ipc_origin) {
                let _ = proxy.send_event(request.body().to_owned());
            }
        });
    let webview = build_webview(builder, &window)?;
    profile::prepare(&webview, &web_context)?;
    webview.load_url(seed.as_str())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut verification_deadline = None;
    let mut verification_started = false;
    let mut challenge_opened = false;
    let mut fetching = false;
    event_loop.run(move |event, _, control_flow| {
        let _keep_context_alive = &web_context;
        let result = match event {
            Event::UserEvent(message) => match serde_json::from_str::<Message>(&message) {
                Ok(Message::Ready) if !fetching => {
                    verification_deadline = None;
                    fetching = true;
                    webview.evaluate_script(&script).err().map(|_| failure())
                }
                Ok(Message::VerificationRequired) if !verification_started => {
                    verification_started = true;
                    verification_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(90));
                    window.set_visible(true);
                    None
                }
                Ok(Message::VerificationRequired | Message::VerificationFailed | Message::Error) => Some(failure()),
                Ok(Message::Challenge) if !verification_started && !challenge_opened => {
                    fetching = false;
                    challenge_opened = true;
                    verification_deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(90));
                    window.set_visible(true);
                    webview.load_url(target.as_str()).err().map(|_| failure())
                }
                Ok(Message::Challenge) => Some(failure()),
                Ok(Message::Fetched { status, content_type, url, body }) => Some(Response::Fetched { status, content_type, url, body }),
                _ => None,
            },
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => Some(failure()),
            Event::NewEvents(_) if Instant::now() >= verification_deadline.unwrap_or(deadline).min(deadline) => Some(failure()),
            _ => None,
        };
        if let Some(result) = result {
            write_output(result);
            *control_flow = ControlFlow::Exit;
        } else {
            *control_flow = ControlFlow::WaitUntil(verification_deadline.unwrap_or(deadline).min(deadline));
        }
    })
}

fn failure() -> Response {
    Response::Error { message: ERROR.into() }
}

fn fetch_script(input: &Request) -> Result<String> {
    let url = serde_json::to_string(&input.url)?;
    let authorization = serde_json::to_string(&input.authorization)?;
    Ok(format!(
        r#"
(async () => {{
    try {{
        const authorization = {authorization};
        const response = await fetch({url}, {{credentials: 'include', headers: authorization ? {{Authorization: authorization}} : {{}}}});
        const contentType = response.headers.get('content-type');
        const bytes = new Uint8Array(await response.arrayBuffer());
        const htmlChallenge = response.status === 403 && contentType?.toLowerCase().includes('text/html') &&
            /challenges\.cloudflare\.com|Just a moment/i.test(new TextDecoder().decode(bytes));
        if (response.headers.get('cf-mitigated') === 'challenge' || htmlChallenge) {{
            window.ipc.postMessage(JSON.stringify({{kind: 'challenge'}}));
            return;
        }}
        const chunks = [];
        for (let start = 0; start < bytes.length; start += 32768) chunks.push(String.fromCharCode(...bytes.subarray(start, start + 32768)));
        window.ipc.postMessage(JSON.stringify({{kind: 'fetched', status: response.status, content_type: contentType, url: response.url, body: btoa(chunks.join(''))}}));
    }} catch (error) {{
        window.ipc.postMessage(JSON.stringify({{kind: 'error'}}));
    }}
}})();
"#
    ))
}

fn write_output(response: Response) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &response);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

#[cfg(target_os = "linux")]
fn build_webview<'a>(builder: WebViewBuilder<'a>, window: &'a tao::window::Window) -> wry::Result<wry::WebView> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;
    let container = window.default_vbox().ok_or_else(|| wry::Error::Io(std::io::Error::other("Tao window has no GTK container")))?;
    builder.build_gtk(container)
}

#[cfg(not(target_os = "linux"))]
fn build_webview<'a>(builder: WebViewBuilder<'a>, window: &'a tao::window::Window) -> wry::Result<wry::WebView> {
    builder.build(window)
}
