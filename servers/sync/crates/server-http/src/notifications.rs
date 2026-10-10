use crate::extract::AuthUser;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{StatusCode};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sync_common::api::notifications::NotificationMessage;
use tokio::sync::broadcast;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const ACCOUNT_CHANNEL_CAPACITY: usize = 256;
const MAX_CONTROL_MESSAGE_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationConfig {
    heartbeat: Duration,
    timeout: Duration,
    auth_revalidation: Duration,
    max_connections: usize,
    max_connections_per_account: usize,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { heartbeat: Duration::from_secs(60), timeout: Duration::from_secs(150), auth_revalidation: Duration::from_secs(30), max_connections: 100_000, max_connections_per_account: 16 }
    }
}

impl NotificationConfig {
    pub fn from_env() -> Result<Self, String> {
        let defaults = Self::default();
        let heartbeat_seconds = crate::parse_bounded("BOKHEIM_NOTIFICATION_HEARTBEAT_SECS", defaults.heartbeat.as_secs(), 15, 300)?;
        let timeout_seconds = crate::parse_bounded("BOKHEIM_NOTIFICATION_TIMEOUT_SECS", defaults.timeout.as_secs(), heartbeat_seconds.saturating_mul(2), 900)?;
        let auth_revalidation_seconds = crate::parse_bounded("BOKHEIM_NOTIFICATION_AUTH_REVALIDATION_SECS", defaults.auth_revalidation.as_secs(), 5, 300)?;
        let max_connections = crate::parse_bounded("BOKHEIM_MAX_NOTIFICATION_CONNECTIONS", defaults.max_connections, 1, 1_000_000)?;
        let max_connections_per_account = crate::parse_bounded("BOKHEIM_MAX_NOTIFICATION_CONNECTIONS_PER_ACCOUNT", defaults.max_connections_per_account, 1, 256)?;
        Ok(Self { heartbeat: Duration::from_secs(heartbeat_seconds), timeout: Duration::from_secs(timeout_seconds), auth_revalidation: Duration::from_secs(auth_revalidation_seconds), max_connections, max_connections_per_account })
    }

    pub fn heartbeat(self) -> Duration {
        self.heartbeat
    }

    pub fn timeout(self) -> Duration {
        self.timeout
    }

    pub fn auth_revalidation(self) -> Duration {
        self.auth_revalidation
    }
}

#[derive(Clone)]
pub(crate) struct NotificationHub {
    accounts: Arc<Mutex<HashMap<String, AccountChannel>>>,
    connections: Arc<Semaphore>,
    config: NotificationConfig,
}

#[derive(Clone, Debug)]
struct HubEvent {
    origin_credential: Option<Arc<str>>,
    message: NotificationMessage,
}

struct AccountChannel {
    sender: broadcast::Sender<HubEvent>,
    connections: usize,
}

struct NotificationSubscription {
    user_id: String,
    accounts: Arc<Mutex<HashMap<String, AccountChannel>>>,
    receiver: broadcast::Receiver<HubEvent>,
    _global_permit: OwnedSemaphorePermit,
}

impl NotificationSubscription {
    async fn recv(&mut self) -> Result<HubEvent, broadcast::error::RecvError> {
        self.receiver.recv().await
    }
}

impl Drop for NotificationSubscription {
    fn drop(&mut self) {
        let mut accounts = self.accounts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let remove = if let Some(channel) = accounts.get_mut(&self.user_id) {
            channel.connections = channel.connections.saturating_sub(1);
            channel.connections == 0
        } else {
            false
        };
        if remove {
            accounts.remove(&self.user_id);
        }
    }
}

impl NotificationHub {
    pub(crate) fn new(config: NotificationConfig) -> Self {
        Self { accounts: Arc::new(Mutex::new(HashMap::new())), connections: Arc::new(Semaphore::new(config.max_connections)), config }
    }

    fn subscribe(&self, user_id: &str) -> Option<NotificationSubscription> {
        let permit = self.connections.clone().try_acquire_owned().ok()?;
        let mut accounts = self.accounts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let channel = accounts.entry(user_id.to_owned()).or_insert_with(|| AccountChannel { sender: broadcast::channel(ACCOUNT_CHANNEL_CAPACITY).0, connections: 0 });
        if channel.connections >= self.config.max_connections_per_account {
            return None;
        }
        channel.connections += 1;
        let receiver = channel.sender.subscribe();
        Some(NotificationSubscription { user_id: user_id.to_owned(), accounts: self.accounts.clone(), receiver, _global_permit: permit })
    }

    pub(crate) fn publish(&self, user_id: &str, origin_credential: Option<Arc<str>>, message: NotificationMessage) {
        let mut accounts = self.accounts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(channel) = accounts.get(user_id) else { return };
        if channel.connections == 0 {
            accounts.remove(user_id);
            return;
        }
        let _ = channel.sender.send(HubEvent { origin_credential, message });
    }
}

pub(crate) async fn connect(State(state): State<AppState>, user: AuthUser, upgrade: WebSocketUpgrade) -> Result<Response, StatusCode> {
    let subscription = state.notifications.subscribe(&user.user_id).ok_or(StatusCode::TOO_MANY_REQUESTS)?;
    let config = state.notifications.config;
    let account = state.account.clone();
    Ok(upgrade.protocols(["bokheim.notifications.v1"]).max_message_size(MAX_CONTROL_MESSAGE_BYTES).max_frame_size(MAX_CONTROL_MESSAGE_BYTES).on_upgrade(move |socket| serve(socket, subscription, account, user.user_id, user.credential, config)))
}

#[path = "notification_auth.rs"]
mod auth;
pub(crate) use auth::websocket_bearer;

enum SocketAction {
    Send(Message),
    Reply(Message),
    Revalidate,
    Ignore,
    Stop,
}

async fn serve(socket: WebSocket, mut notifications: NotificationSubscription, account: Arc<server_account::PostgresAccountService>, user_id: String, credential: Arc<str>, config: NotificationConfig) {
    let (mut outgoing, mut incoming) = socket.split();
    let hello = NotificationMessage::Hello { heartbeat_seconds: config.heartbeat.as_secs(), timeout_seconds: config.timeout.as_secs() };
    let Ok(bytes) = wire::encode(&hello) else { return };
    if outgoing.send(Message::Binary(bytes.into())).await.is_err() {
        return;
    }

    let mut heartbeat = tokio::time::interval(config.heartbeat);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut auth_revalidation = tokio::time::interval(config.auth_revalidation);
    auth_revalidation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    auth_revalidation.tick().await;
    let mut last_client_activity = Instant::now();

    loop {
        let action = tokio::select! {
            incoming = incoming.next() => match incoming {
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                    last_client_activity = Instant::now();
                    SocketAction::Ignore
                }
                Some(Ok(Message::Ping(payload))) => {
                    last_client_activity = Instant::now();
                    SocketAction::Reply(Message::Pong(payload))
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => SocketAction::Stop,
            },
            event = notifications.recv() => match event {
                Ok(event) if event.origin_credential.as_deref() == Some(credential.as_ref()) => SocketAction::Ignore,
                Ok(event) => match wire::encode(&event.message) {
                    Ok(bytes) => SocketAction::Send(Message::Binary(bytes.into())),
                    Err(_) => SocketAction::Stop,
                },
                Err(broadcast::error::RecvError::Lagged(_)) => match wire::encode(&NotificationMessage::Reconcile) {
                    Ok(bytes) => SocketAction::Send(Message::Binary(bytes.into())),
                    Err(_) => SocketAction::Stop,
                },
                Err(broadcast::error::RecvError::Closed) => SocketAction::Stop,
            },
            _ = heartbeat.tick() => {
                if last_client_activity.elapsed() >= config.timeout {
                    SocketAction::Stop
                } else {
                    SocketAction::Send(Message::Binary(Vec::new()))
                }
            },
            _ = auth_revalidation.tick() => SocketAction::Revalidate,
        };

        match action {
            SocketAction::Send(message) => {
                if !credential_is_current(&account, &user_id, &credential).await {
                    break;
                }
                if outgoing.send(message).await.is_err() {
                    break;
                }
            }
            SocketAction::Reply(message) => {
                if outgoing.send(message).await.is_err() {
                    break;
                }
            }
            SocketAction::Revalidate => {
                if !credential_is_current(&account, &user_id, &credential).await {
                    break;
                }
            }
            SocketAction::Ignore => {}
            SocketAction::Stop => break,
        }
    }
    let _ = outgoing.send(Message::Close(None)).await;
}

async fn credential_is_current(account: &server_account::PostgresAccountService, expected_user_id: &str, credential: &str) -> bool {
    account.resolve_token(credential).await.is_ok_and(|user| user.user_id == expected_user_id)
}
