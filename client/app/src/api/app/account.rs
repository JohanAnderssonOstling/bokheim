use super::AppClient;
use crate::runtime::AccountSnapshot;
use crate::runtime::AppCommand;
use crate::BackendError;

impl AppClient {
    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        self.request(AppCommand::AccountSnapshot).await
    }
    pub async fn local_storage_usage(&self) -> Result<Vec<crate::LocalLibraryStorageUsage>, BackendError> {
        self.request(AppCommand::LocalStorageUsage).await
    }
    pub async fn login(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.request(AppCommand::Login { email, password }).await
    }

    pub async fn verify_email(&self, email: String, pin: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.request(AppCommand::VerifyEmail { email, pin, password }).await
    }

    pub async fn logout(&self) -> Result<AccountSnapshot, BackendError> {
        self.request(AppCommand::Logout).await
    }

    pub async fn delete_account(&self, email: String, password: String) -> Result<AccountSnapshot, BackendError> {
        self.request(AppCommand::DeleteAccount { email, password }).await
    }
    pub async fn request_public_registration(&self, email: String, password: String) -> Result<(), BackendError> {
        self.request(AppCommand::RequestPublicRegistration { email, password }).await
    }

    pub async fn resend_verification(&self, email: String) -> Result<(), BackendError> {
        self.request(AppCommand::ResendVerification { email }).await
    }

    pub async fn request_password_reset(&self, email: String) -> Result<(), BackendError> {
        self.request(AppCommand::RequestPasswordReset { email }).await
    }

    pub async fn reset_password(&self, token: String, password: String) -> Result<(), BackendError> {
        self.request(AppCommand::ResetPassword { token, password }).await
    }
}
