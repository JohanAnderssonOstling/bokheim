//! Owns authentication inputs and sign-in, registration, verification, and reset flows.

use ui_text::text;

use gpui::{AppContext, Context, Entity, Window};
use gpui_component::input::{InputState, OtpState};
use std::time::Duration;

use crate::model::AccountFlow;
use account_client::api_server_url;
use library_backend::BackendError;

use super::SyncPage;

pub(super) struct AuthenticationInputs {
    pub(super) login_email: Entity<InputState>,
    pub(super) password: Entity<InputState>,
    pub(super) email: Entity<InputState>,
    pub(super) registration_password: Entity<InputState>,
    pub(super) verification_pin: Entity<OtpState>,
    pub(super) reset_token: Entity<InputState>,
    pub(super) new_password: Entity<InputState>,
    pub(super) resend_seconds_remaining: u32,
    resend_task: Option<gpui::Task<()>>,
}

impl AuthenticationInputs {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SyncPage>) -> Self {
        let verification_pin = cx.new(|cx| OtpState::new(6, window, cx));
        let mut input = |placeholder: &'static str, masked: bool, cx: &mut Context<SyncPage>| cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).masked(masked));
        Self {
            login_email: input(text!(account::email), false, cx),
            password: input(text!(common::password), true, cx),
            email: input(text!(account::email), false, cx),
            registration_password: input(text!(common::password), true, cx),
            verification_pin,
            reset_token: input(text!(account::reset_code_from_your_email), false, cx),
            new_password: input(text!(account::new_password), true, cx),
            resend_seconds_remaining: 0,
            resend_task: None,
        }
    }
}

impl SyncPage {
    /// Leaving a flow re-masks every password, so a field revealed on one
    /// screen is never left in clear text behind another.
    pub(super) fn set_flow(&mut self, flow: AccountFlow, cx: &mut Context<Self>) {
        if self.flow == flow && self.error.is_none() && self.message.is_none() {
            return;
        }
        for state in [&self.authentication.password, &self.authentication.registration_password, &self.authentication.new_password] {
            state.update(cx, |state, cx| state.set_masked(true, cx));
        }
        self.flow = flow;
        self.login_failed = false;
        self.error = None;
        self.message = None;
        cx.notify();
    }

    fn start_account_request(&mut self, progress: &'static str, success: &'static str, next_flow: AccountFlow, operation: impl Future<Output = Result<(), BackendError>> + 'static, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        self.activity = super::AccountActivity::AccountRequest;
        self.error = None;
        self.message = Some(progress.into());
        cx.notify();
        self.task = Some(cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                page.activity = super::AccountActivity::Idle;
                match result {
                    Ok(()) => {
                        if next_flow == AccountFlow::VerifyEmail {
                            page.start_resend_cooldown(cx);
                        }
                        page.set_flow(next_flow, cx);
                        page.message = Some(success.into());
                    }
                    Err(error) => {
                        page.message = None;
                        page.error = Some(page.authentication_error(&error).into());
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
            self.error = Some(text!(account::enter_your_email_address_and_password).into());
            cx.notify();
            return;
        }
        let backend = self.services.backend.clone();
        let operation = async move { backend.account.login(email, password).await };
        self.start_snapshot_operation(text!(account::signing_in), None, true, operation, cx);
    }

    pub(super) fn start_registration(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        let password = self.authentication.registration_password.read(cx).value().to_string();
        if email.is_empty() || password.chars().count() < account_contract::PASSWORD_MIN_CHARACTERS {
            self.error = Some(format!(text!(account::enter_your_email_address_and_use_a_password_with_at_least_characters), account_contract::PASSWORD_MIN_CHARACTERS).into());
            cx.notify();
            return;
        }
        let operation = async move { account_client::request_public_registration(&api_server_url(), &email, &password).await.map_err(BackendError::operation) };
        self.start_account_request(text!(account::creating_account), text!(account::check_your_email_for_a_six_digit_verification_pin), AccountFlow::VerifyEmail, operation, cx);
    }

    pub(super) fn start_resend_verification(&mut self, cx: &mut Context<Self>) {
        if self.authentication.resend_seconds_remaining > 0 {
            return;
        }
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        if email.is_empty() {
            self.error = Some(text!(account::enter_your_email_address).into());
            cx.notify();
            return;
        }
        let operation = async move { account_client::resend_verification(&api_server_url(), &email).await.map_err(BackendError::operation) };
        self.start_account_request(text!(account::resending_pin), text!(account::a_new_pin_has_been_requested_check_your_email_and_use_the), AccountFlow::VerifyEmail, operation, cx);
    }

    fn start_resend_cooldown(&mut self, cx: &mut Context<Self>) {
        self.authentication.resend_seconds_remaining = account_contract::EMAIL_RESEND_COOLDOWN_SECONDS;
        let executor = cx.background_executor().clone();
        self.authentication.resend_task = Some(cx.spawn(async move |page, cx| {
            loop {
                executor.timer(Duration::from_secs(1)).await;
                let remaining = page.update(cx, |page, cx| {
                    page.authentication.resend_seconds_remaining = page.authentication.resend_seconds_remaining.saturating_sub(1);
                    cx.notify();
                    page.authentication.resend_seconds_remaining
                });
                if !matches!(remaining, Ok(remaining) if remaining > 0) {
                    break;
                }
            }
        }));
    }

    pub(super) fn start_verify_email(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        let pin = self.authentication.verification_pin.read(cx).value().trim().to_owned();
        let password = self.authentication.registration_password.read(cx).value().to_string();
        let pin_digits = account_contract::EMAIL_VERIFICATION_PIN_DIGITS;
        if email.is_empty() {
            self.error = Some(text!(account::enter_your_email_address).into());
            cx.notify();
            return;
        }
        if pin.len() != pin_digits || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
            self.error = Some(format!(text!(account::enter_the_digit_verification_pin_from_your_email), pin_digits = pin_digits).into());
            cx.notify();
            return;
        }
        if password.is_empty() {
            self.error = Some(text!(account::your_signup_details_are_missing_start_account_creation_again).into());
            cx.notify();
            return;
        }
        let backend = self.services.backend.clone();
        let operation = async move { backend.account.verify_email(email, pin, password).await };
        self.start_snapshot_operation(text!(account::verifying_email), Some(text!(account::email_verified)), false, operation, cx);
    }

    pub(super) fn start_password_reset_request(&mut self, cx: &mut Context<Self>) {
        let email = self.authentication.email.read(cx).value().trim().to_owned();
        if email.is_empty() {
            self.error = Some(text!(account::enter_your_email_address).into());
            cx.notify();
            return;
        }
        let operation = async move { account_client::request_password_reset(&api_server_url(), &email).await.map_err(BackendError::operation) };
        self.start_account_request(text!(account::requesting_reset), text!(account::check_your_email_for_a_password_reset_code_if_the_address_has), AccountFlow::ResetPassword, operation, cx);
    }

    pub(super) fn start_password_reset(&mut self, cx: &mut Context<Self>) {
        let token = self.authentication.reset_token.read(cx).value().trim().to_owned();
        let password = self.authentication.new_password.read(cx).value().to_string();
        if token.is_empty() || password.chars().count() < account_contract::PASSWORD_MIN_CHARACTERS {
            self.error = Some(format!(text!(account::enter_the_reset_code_from_your_email_and_use_a_password_with), account_contract::PASSWORD_MIN_CHARACTERS).into());
            cx.notify();
            return;
        }
        let operation = async move { account_client::reset_password(&api_server_url(), &token, &password).await.map_err(BackendError::operation) };
        self.start_account_request(text!(account::updating_password), text!(account::password_updated_sign_in_with_your_new_password), AccountFlow::SignIn, operation, cx);
    }
}
