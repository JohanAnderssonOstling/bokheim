//! Owns authentication inputs and sign-in, registration, verification, and reset flows.

use std::future::Future;

use epub_svelte_lib::app::AppBackend;
use gpui::{AppContext, Context, Entity, Window};
use gpui_component::input::{InputState, OtpState};

use crate::model::AccountFlow;

use super::SyncPage;
use super::account::load_cloud_snapshot;

pub(super) struct AuthenticationInputs {
    pub(super) login_email: Entity<InputState>,
    pub(super) password: Entity<InputState>,
    pub(super) email: Entity<InputState>,
    pub(super) registration_password: Entity<InputState>,
    pub(super) verification_pin: Entity<OtpState>,
    pub(super) reset_token: Entity<InputState>,
    pub(super) new_password: Entity<InputState>,
}

impl AuthenticationInputs {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SyncPage>) -> Self {
        let verification_pin = cx.new(|cx| OtpState::new(6, window, cx));
        let mut input = |placeholder: &'static str, masked: bool, cx: &mut Context<SyncPage>| cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).masked(masked));
        Self {
            login_email: input("Email", false, cx),
            password: input("Password", true, cx),
            email: input("Email", false, cx),
            registration_password: input("Password", true, cx),
            verification_pin,
            reset_token: input("Reset token", false, cx),
            new_password: input("New password", true, cx),
        }
    }
}

impl SyncPage {
    pub(super) fn set_flow(&mut self, flow: AccountFlow, cx: &mut Context<Self>) {
        if self.flow == flow && self.error.is_none() && self.message.is_none() {
            return;
        }
        self.flow = flow;
        self.login_failed = false;
        self.error = None;
        self.message = None;
        cx.notify();
    }

    fn start_account_request<F, Fut>(&mut self, progress: &'static str, success: &'static str, next_flow: AccountFlow, operation: F, cx: &mut Context<Self>)
    where
        F: FnOnce(AppBackend) -> Fut,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        if self.busy {
            return;
        }
        self.busy = true;
        self.account_action_busy = true;
        self.error = None;
        self.message = Some(progress.into());
        cx.notify();
        let task = self.services.backend.spawn(operation);
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = task.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = page.update(cx, |page, cx| {
                page.busy = false;
                page.account_action_busy = false;
                match result {
                    Ok(()) => {
                        page.flow = next_flow;
                        page.message = Some(success.into());
                    }
                    Err(error) => {
                        page.message = None;
                        page.error = Some(error.into());
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn start_login(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.login_email.read(cx).value().trim().to_owned();
        let password = self.authentication.password.read(cx).value().to_string();
        if email.is_empty() || password.is_empty() {
            self.error = Some("Email and password are required".into());
            cx.notify();
            return;
        }
        self.start_snapshot_operation(
            "Signing in…",
            "Signed in",
            true,
            move |core| async move {
                core.login(&email, &password).await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }

    pub(super) fn start_registration(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        let password = self.authentication.registration_password.read(cx).value().to_string();
        let validation = &self.services.contracts.account.validation;
        if email.is_empty() || password.chars().count() < usize::from(validation.password_min_characters) {
            self.error = Some("Check the email and password length".into());
            cx.notify();
            return;
        }
        self.start_account_request(
            "Creating account…",
            "Check your email for a six-digit verification PIN",
            AccountFlow::VerifyEmail,
            move |core| async move { core.request_public_registration(&email, &password).await.map_err(|error| error.to_string()) },
            cx,
        );
    }

    pub(super) fn start_resend_verification(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        if email.is_empty() {
            self.error = Some("Email is required".into());
            cx.notify();
            return;
        }
        self.start_account_request("Resending PIN…", "Verification PIN sent if the address exists", AccountFlow::VerifyEmail, move |core| async move { core.resend_verification(&email).await.map_err(|error| error.to_string()) }, cx);
    }

    pub(super) fn start_verify_email(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        let pin = self.authentication.verification_pin.read(cx).value().trim().to_owned();
        let pin_digits = usize::from(self.services.contracts.account.validation.verification_pin_digits);
        if email.is_empty() || pin.len() != pin_digits || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
            self.error = Some(format!("Enter the {pin_digits}-digit verification PIN").into());
            cx.notify();
            return;
        }
        self.start_snapshot_operation(
            "Verifying email…",
            "Email verified",
            false,
            move |core| async move {
                core.verify_email(&email, &pin).await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }

    pub(super) fn start_password_reset_request(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        if email.is_empty() {
            self.error = Some("Email is required".into());
            cx.notify();
            return;
        }
        self.start_account_request("Requesting reset…", "Reset token sent if the address exists", AccountFlow::ResetPassword, move |core| async move { core.request_password_reset(&email).await.map_err(|error| error.to_string()) }, cx);
    }

    pub(super) fn start_password_reset(&mut self, cx: &mut Context<Self>) {
        let token = self.authentication.reset_token.read(cx).value().trim().to_owned();
        let password = self.authentication.new_password.read(cx).value().to_string();
        if token.is_empty() || password.chars().count() < usize::from(self.services.contracts.account.validation.password_min_characters) {
            self.error = Some("Reset token and a long enough password are required".into());
            cx.notify();
            return;
        }
        self.start_account_request(
            "Updating password…",
            "Password updated. Sign in with your new password.",
            AccountFlow::SignIn,
            move |core| async move { core.reset_password(&token, &password).await.map_err(|error| error.to_string()) },
            cx,
        );
    }
}
