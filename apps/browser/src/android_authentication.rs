//! Android UI transport. Session IDs prevent a dismissed screen receiving a
//! later account response. Credentials exist only in the bounded request queue.
use ui_text::text;
use operation_failure::OperationError;

use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex, MutexGuard, OnceLock,
    atomic::{AtomicU64, Ordering},
};
type Transport = dyn Fn(&str) -> Result<(), OperationError> + Send + Sync;
static TRANSPORT: OnceLock<Arc<Transport>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE: Mutex<Option<(u64, Sender<Request>)>> = Mutex::new(None);

fn active_session() -> MutexGuard<'static, Option<(u64, Sender<Request>)>> {
    match ACTIVE.lock() {
        Ok(active) => active,
        Err(error) => {
            log::error!("Discarding failed Android authentication session");
            let mut active = error.into_inner();
            *active = None;
            ACTIVE.clear_poison();
            active
        }
    }
}

/// A message from the Android dialog: `"closed"`, or `{"submit": {...}}`.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Request {
    Closed,
    Submit(crate::AuthenticationRequest),
}

pub fn initialize(transport: impl Fn(&str) -> Result<(), OperationError> + Send + Sync + 'static) {
    let _ = TRANSPORT.set(Arc::new(transport));
}

/// Called by JNI; never log the message because it contains credentials.
pub fn receive(id: u64, message: &str) {
    let Ok(request) = serde_json::from_str::<Request>(message) else {
        // Release the dialog's busy state rather than leaving it waiting.
        Session::complete(id, Some(text!(browse::could_not_read_the_sign_in_form_close_it_and_try_again)));
        return;
    };
    let active = active_session();
    if let Some((active_id, sender)) = active.as_ref() {
        if *active_id == id {
            let _ = sender.try_send(request);
        }
    }
}
fn send(message: serde_json::Value) -> Result<(), OperationError> {
    TRANSPORT.get().ok_or("Android authentication is unavailable")?(&message.to_string())
}
/// Named ARGB colors for the native account form's JSON boundary.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Theme {
    pub background: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub border: u32,
    pub link: u32,
    pub danger: u32,
    pub selection: u32,
}

pub(crate) struct Session(pub u64);
impl Session {
    pub fn open(theme: Theme) -> Result<(Self, Receiver<Request>), OperationError> {
        let session = Self(NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let (sender, receiver) = async_channel::bounded(2);
        *active_session() = Some((session.0, sender));
        send(serde_json::json!({"command":"open", "id":session.0, "theme":theme, "minimum":account_contract::PASSWORD_MIN_CHARACTERS, "resendCooldownSeconds":account_contract::EMAIL_RESEND_COOLDOWN_SECONDS}))?;
        Ok((session, receiver))
    }
    pub fn complete(id: u64, error: Option<&str>) {
        let _ = send(serde_json::json!({"command":"result", "id":id, "error":error}));
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let mut active = active_session();
        if active.as_ref().is_some_and(|(id, _)| *id == self.0) {
            *active = None;
        }
        drop(active);
        let _ = send(serde_json::json!({"command":"close", "id":self.0}));
    }
}
