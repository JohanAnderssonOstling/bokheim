//! Automatic local sign-in storage and filling, with no browser controls.
use super::Action;
use crate::passwords::{self, Command, Login, Reply, SavedLogin, Vault};
use serde::Deserialize;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};
use wry::{WebView, http::Request};
use zeroize::Zeroizing;

type Session = Rc<RefCell<Option<(String, String)>>>;

#[derive(Deserialize)]
pub(super) struct Message {
    nonce: String,
    origin: String,
    #[serde(flatten)]
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Operation {
    Fill { username: String },
    Submitted { attempt: String, username: String, password: Zeroizing<String> },
    FormGone { attempt: String },
}

struct Pending {
    attempt: String,
    login: Login,
    expires: Instant,
}

pub(super) struct Autofill {
    pub guard: Session,
    worker: Sender<(String, Command)>,
    pending: Option<Pending>,
}

fn decode(source: &str, body: String, session: &Option<(String, String)>) -> Option<Message> {
    let body = Zeroizing::new(body);
    if body.len() > 16_384 {
        return None;
    }
    let message: Message = serde_json::from_str(&body).ok()?;
    let (nonce, origin) = session.as_ref()?;
    // Linux reports the top-level URL for iframe IPC too. Only the main
    // document has this fresh per-document nonce; a child cannot read it.
    (message.nonce == *nonce && message.origin == *origin && passwords::origin(source).as_deref() == Some(origin.as_str())).then_some(message)
}

impl Autofill {
    pub(super) fn new(directory: PathBuf, sender: Sender<Action>) -> Self {
        let (worker, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let vault = Vault::new(directory);
            while let Ok((nonce, command)) = receiver.recv() {
                let result = vault.execute(command);
                if sender.send(Action::PasswordResult { nonce, result }).is_err() {
                    break;
                }
            }
        });
        Self { guard: Rc::default(), worker, pending: None }
    }

    pub(super) fn handler(&self, sender: Sender<Action>) -> impl Fn(Request<String>) + 'static {
        let guard = self.guard.clone();
        move |request| {
            let (parts, body) = request.into_parts();
            if let Some(message) = decode(&parts.uri.to_string(), body, &guard.borrow()) {
                let _ = sender.send(Action::PasswordMessage(message));
            }
        }
    }

    pub(super) fn install(&mut self, page: &WebView) {
        let Some(origin) = page.url().ok().and_then(|url| passwords::origin(&url)) else {
            self.cancel();
            return;
        };
        self.expire();
        if self.pending.as_ref().is_some_and(|pending| pending.login.site.origin != origin) {
            self.cancel();
        }
        let mut random = [0; 32];
        if getrandom::fill(&mut random).is_err() {
            return;
        }
        let nonce = blake3::hash(&random).to_hex().to_string();
        let attempt = self.pending.as_ref().map(|pending| pending.attempt.clone());
        let request = serde_json::json!({"kind": "install", "origin": origin, "nonce": nonce, "attempt": attempt});
        *self.guard.borrow_mut() = Some((nonce, origin));
        if page.evaluate_script(&format!("({})({request})", include_str!("../passwords.js"))).is_err() {
            self.guard.borrow_mut().take();
        }
    }

    pub(super) fn receive(&mut self, message: Message) {
        self.expire();
        let command = match message.operation {
            Operation::Fill { username } => Command::Lookup { origin: message.origin, username },
            Operation::Submitted { attempt, username, password } => {
                if !attempt.starts_with(&format!("{}:", message.nonce)) || attempt.len() > 128 || username.len() > 4096 || password.is_empty() || password.len() > 2560 {
                    return;
                }
                self.pending = Some(Pending { attempt, login: Login { site: SavedLogin { origin: message.origin, username }, password }, expires: Instant::now() + Duration::from_secs(60) });
                return;
            }
            Operation::FormGone { attempt } => {
                if !self.guard.borrow().as_ref().is_some_and(|(nonce, origin)| *nonce == message.nonce && *origin == message.origin) {
                    return;
                }
                if !self.pending.as_ref().is_some_and(|pending| pending.attempt == attempt && pending.login.site.origin == message.origin) {
                    return;
                }
                Command::Save(self.pending.take().unwrap().login)
            }
        };
        let _ = self.worker.send((message.nonce, command));
    }

    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    pub(super) fn expire(&mut self) {
        if self.pending.as_ref().is_some_and(|pending| Instant::now() >= pending.expires) {
            self.cancel();
        }
    }

    pub(super) fn apply(&self, page: Option<&WebView>, nonce: &str, result: Result<Reply, String>) {
        match result {
            Ok(Reply::Match(Some(login))) => {
                if self.guard.borrow().as_ref().is_some_and(|(current, origin)| current == nonce && *origin == login.site.origin)
                    && let Some(page) = page
                    && page.url().ok().and_then(|url| passwords::origin(&url)).as_deref() == Some(login.site.origin.as_str())
                {
                    let _ = page.evaluate_script(&passwords::fill_script(&login, nonce));
                }
            }
            Err(error) => eprintln!("Browser password storage: {error}"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_stays_in_memory_until_the_matching_document_confirms_disappearance() {
        let (worker, requests) = mpsc::channel();
        let mut autofill = Autofill { guard: Rc::default(), worker, pending: None };
        let origin = "https://example.com".to_owned();
        autofill.receive(Message { nonce: "source".into(), origin: origin.clone(), operation: Operation::Submitted { attempt: "source:1".into(), username: "reader".into(), password: Zeroizing::new("replacement-secret".into()) } });
        assert!(requests.try_recv().is_err(), "a submit must never write to the credential store");
        *autofill.guard.borrow_mut() = Some(("result".into(), origin.clone()));
        for (nonce, attempt) in [("source", "source:1"), ("result", "older-attempt")] {
            autofill.receive(Message { nonce: nonce.into(), origin: origin.clone(), operation: Operation::FormGone { attempt: attempt.into() } });
            assert!(requests.try_recv().is_err());
        }
        autofill.receive(Message { nonce: "result".into(), origin, operation: Operation::FormGone { attempt: "source:1".into() } });
        let (_, Command::Save(login)) = requests.try_recv().unwrap() else { panic!() };
        assert_eq!(login.password.as_str(), "replacement-secret");
        assert!(autofill.pending.is_none());
    }

    #[test]
    fn expired_and_cancelled_attempts_cannot_replace_a_saved_password() {
        let (worker, requests) = mpsc::channel();
        let origin = "https://example.com".to_owned();
        let mut autofill = Autofill { guard: Rc::new(RefCell::new(Some(("result".into(), origin.clone())))), worker, pending: None };
        for cancel in [false, true] {
            autofill.receive(Message { nonce: "source".into(), origin: origin.clone(), operation: Operation::Submitted { attempt: "source:1".into(), username: "reader".into(), password: Zeroizing::new("unconfirmed-secret".into()) } });
            if cancel {
                autofill.cancel();
            } else {
                autofill.pending.as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
            }
            autofill.receive(Message { nonce: "result".into(), origin: origin.clone(), operation: Operation::FormGone { attempt: "source:1".into() } });
            assert!(requests.try_recv().is_err());
            assert!(autofill.pending.is_none());
        }
    }

    #[test]
    fn iframe_forgery_and_stale_documents_cannot_access_passwords() {
        let session = Some(("current-nonce".to_owned(), "https://example.com".to_owned()));
        let body = |nonce, origin| serde_json::json!({"nonce": nonce, "origin": origin, "kind": "fill", "username": "reader"}).to_string();
        assert!(decode("https://example.com/login", body("current-nonce", "https://example.com"), &session).is_some());
        assert!(decode("https://example.com/login", body("stale-nonce", "https://example.com"), &session).is_none());
        assert!(decode("https://example.com/login", body("current-nonce", "https://other.test"), &session).is_none());
        assert!(decode("https://other.test/login", body("current-nonce", "https://example.com"), &session).is_none());
        assert!(decode("https://example.com/login", body("current-nonce", "https://example.com"), &None).is_none());
        assert!(decode("https://example.com/login", "x".repeat(16_385), &session).is_none());
    }
}
