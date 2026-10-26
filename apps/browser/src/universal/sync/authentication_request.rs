//! A submission from a platform sign-in form (the web dialog or the Android
//! dialog). Each form sends JSON that is parsed once, at its bridge, into this
//! enum; every action carries exactly the fields it uses.

use app::AppClient;
use serde::Deserialize;
use ui_components::AccountAction;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AuthenticationRequest {
    Login { email: String, password: String },
    Register { email: String, password: String },
    Verify { email: String, pin: String, password: String },
    Resend { email: String },
    RequestReset { email: String },
    Reset { token: String, password: String },
}

impl AuthenticationRequest {
    pub fn action(&self) -> AccountAction {
        match self {
            Self::Login { .. } => AccountAction::Login,
            Self::Register { .. } => AccountAction::Register,
            Self::Verify { .. } => AccountAction::Verify,
            Self::Resend { .. } => AccountAction::Resend,
            Self::RequestReset { .. } => AccountAction::RequestReset,
            Self::Reset { .. } => AccountAction::Reset,
        }
    }

    /// Whether success leaves this device signed in.
    pub fn signs_in(&self) -> bool {
        matches!(self, Self::Login { .. } | Self::Verify { .. })
    }

    /// Runs the request and returns the user-facing failure message.
    pub async fn submit(self, backend: &AppClient) -> Result<(), &'static str> {
        let action = self.action();
        let result = match self {
            Self::Login { email, password } => backend.login(email, password).await.map(|_| ()),
            Self::Register { email, password } => backend.request_public_registration(email, password).await,
            Self::Verify { email, pin, password } => backend.verify_email(email, pin, password).await.map(|_| ()),
            Self::Resend { email } => backend.resend_verification(email).await,
            Self::RequestReset { email } => backend.request_password_reset(email).await,
            Self::Reset { token, password } => backend.reset_password(token, password).await,
        };
        result.map_err(|error| ui_components::authentication_error(&error, action))
    }
}
