//! Real-browser regressions for one verification attempt and its failure paths.
use library_opds::browser::{Request, Response};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

struct Fixture {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn new(scenario: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/feed.atom", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let received = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                received.fetch_add(1, Ordering::Relaxed);
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&buffer[..count]),
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, headers, body) = match (scenario, path) {
                    ("direct-feed", "/") => ("403 Forbidden", "cf-mitigated: challenge\r\n", "<!doctype html><title>Just a moment...</title><p>The home page requires verification</p>"),
                    ("page-loop", _) => ("403 Forbidden", "cf-mitigated: challenge\r\n", "<!doctype html><title>Just a moment...</title><script>setTimeout(() => location.reload(), 150)</script>"),
                    ("fetch-loop", "/feed.atom") => ("403 Forbidden", "cf-mitigated: challenge\r\n", "<!doctype html><title>Loading</title><p>Just a moment</p>"),
                    ("fetch-loop", _) => ("200 OK", "", "<!doctype html><title>Ready</title>"),
                    ("widget-failure", _) => (
                        "403 Forbidden",
                        "cf-mitigated: challenge\r\n",
                        "<!doctype html><title>Just a moment...</title><script>setTimeout(() => window.dispatchEvent(new MessageEvent('message', {origin: 'https://challenges.cloudflare.com', data: {source: 'cloudflare-challenge', event: 'fail'}})), 150); setTimeout(() => {document.title = 'Ready'}, 600)</script>",
                    ),
                    (_, "/feed.atom") => ("200 OK", "", "<feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Verified</title></feed>"),
                    _ => (
                        "200 OK",
                        "",
                        "<!doctype html><title>Just a moment...</title><div id=\"challenge-running\">Verifying</div><script>setTimeout(() => {window.dispatchEvent(new MessageEvent('message', {origin: location.origin, data: {source: 'cloudflare-challenge', event: 'fail'}})); document.title = 'Ready'; document.querySelector('#challenge-running').remove()}, 150)</script><p>Resource: https://challenges.cloudflare.com</p>",
                    ),
                };
                let content_type = if scenario == "direct-feed" && path == "/feed.atom" { "application/atom+xml" } else { "text/html" };
                let _ = write!(stream, "HTTP/1.1 {status}\r\n{headers}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        Self { url, requests, stop, thread: Some(thread) }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn verify(fixture: &Fixture, direct: bool) -> Response {
    let profile = tempfile::tempdir().unwrap();
    let input = Request { url: fixture.url.clone(), authorization: None, data_directory: profile.path().to_owned(), navigation_url: direct.then(|| fixture.url.clone()) };
    let mut child = Command::new(env!("CARGO_BIN_EXE_bokheim-opds-webview")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&serde_json::to_vec(&input).unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("verification retried or failed to finish");
        }
        thread::sleep(Duration::from_millis(20));
    }
    serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
#[ignore = "requires a desktop session to open the real verification browser"]
fn verification_succeeds_or_stops_after_the_first_failure_without_retries() {
    for scenario in ["same-document", "direct-feed"] {
        let fixture = Fixture::new(scenario);
        assert!(matches!(verify(&fixture, scenario == "direct-feed"), Response::Fetched { status: 200, .. }));
    }
    for (scenario, maximum_requests) in [("page-loop", 2), ("fetch-loop", 4), ("widget-failure", 1)] {
        let fixture = Fixture::new(scenario);
        match verify(&fixture, false) {
            Response::Error { message } => assert_eq!(message, "Browser verification could not be completed for this catalog. Please try another catalog."),
            Response::Fetched { .. } => panic!("a failed verification was retried or returned as a feed"),
        }
        assert!(fixture.requests.load(Ordering::Relaxed) <= maximum_requests, "the failed verification made extra requests");
    }
}
