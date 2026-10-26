//! Google authorization-code flow with PKCE. The host opens the system browser
//! and supplies the callback; access/refresh tokens belong only to this device.

use crate::{CloudError, metadata};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sync_common::storage::ProviderId;

/// Public desktop client credentials supplied by the distribution. Desktop
/// client secrets are not treated as confidential or used for app identity.
#[derive(Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Clone)]
pub struct GoogleOAuth {
    config: GoogleOAuthConfig,
    client: Client,
    authorization: Url,
    token: Url,
    userinfo: Url,
}

/// Neither a pending login nor a token set implements Debug: they contain
/// secrets and must never be logged or included in metadata synchronization.
pub struct PendingGoogleLogin {
    oauth: GoogleOAuth,
    redirect: Url,
    verifier: String,
    state: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GoogleTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleAccount {
    pub id: ProviderId,
    pub email: String,
    pub name: Option<String>,
}

impl GoogleOAuth {
    pub fn new(config: GoogleOAuthConfig) -> Result<Self, CloudError> {
        if config.client_id.trim().is_empty() || config.client_id.chars().any(char::is_control) {
            return Err(CloudError::InvalidRequest("Google Drive sign-in requires a configured OAuth client ID".into()));
        }
        let builder = Client::builder().timeout(std::time::Duration::from_secs(30));
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.redirect(reqwest::redirect::Policy::none());
        Ok(Self {
            config,
            client: builder.build().map_err(CloudError::transport)?,
            authorization: Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("static authorization URL"),
            token: Url::parse("https://oauth2.googleapis.com/token").expect("static token URL"),
            userinfo: Url::parse("https://openidconnect.googleapis.com/v1/userinfo").expect("static user information URL"),
        })
    }

    pub fn begin_login(&self, redirect: Url) -> Result<PendingGoogleLogin, CloudError> {
        // Native hosts use an ephemeral loopback port. Browser hosts use their
        // registered HTTPS callback. Never authorize an arbitrary HTTP origin.
        let loopback = redirect.scheme() == "http" && matches!(redirect.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
        if (!loopback && redirect.scheme() != "https") || !redirect.username().is_empty() || redirect.password().is_some() || redirect.query().is_some() || redirect.fragment().is_some() {
            return Err(CloudError::InvalidRequest("Invalid Google authorization callback".into()));
        }
        Ok(PendingGoogleLogin { oauth: self.clone(), redirect, verifier: random_value()?, state: random_value()? })
    }

    pub async fn refresh(&self, previous: &GoogleTokens, now: u64) -> Result<GoogleTokens, CloudError> {
        let refresh = previous.refresh_token.as_deref().ok_or(CloudError::AuthenticationRequired)?;
        let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", self.config.client_id.as_str())];
        if let Some(secret) = &self.config.client_secret { form.push(("client_secret", secret)); }
        let mut tokens = self.exchange(&form, now).await?;
        if tokens.refresh_token.is_none() { tokens.refresh_token = previous.refresh_token.clone(); }
        Ok(tokens)
    }

    pub async fn account(&self, tokens: &GoogleTokens) -> Result<GoogleAccount, CloudError> {
        #[derive(Deserialize)]
        struct UserInfo { sub: ProviderId, email: String, name: Option<String>, email_verified: bool }
        let response = self.client.get(self.userinfo.clone()).bearer_auth(&tokens.access_token).send().await.map_err(CloudError::transport)?;
        let user: UserInfo = metadata(response).await?;
        if !user.email_verified || user.email.trim().is_empty() { return Err(CloudError::InvalidResponse("Google did not return a verified account".into())); }
        Ok(GoogleAccount { id: user.sub, email: user.email, name: user.name })
    }

    async fn exchange(&self, form: &[(&str, &str)], now: u64) -> Result<GoogleTokens, CloudError> {
        let response = self.client.post(self.token.clone()).form(form).send().await.map_err(CloudError::transport)?;
        if response.status() == reqwest::StatusCode::BAD_REQUEST { return Err(CloudError::AuthenticationRequired); }
        #[derive(Deserialize)]
        struct TokenResponse { access_token: String, refresh_token: Option<String>, expires_in: u64, token_type: String }
        let token: TokenResponse = metadata(response).await?;
        if token.access_token.is_empty() || token.access_token.len() > 32 * 1024 || token.expires_in == 0 || !token.token_type.eq_ignore_ascii_case("bearer") {
            return Err(CloudError::InvalidResponse("Invalid Google authorization token response".into()));
        }
        Ok(GoogleTokens { access_token: token.access_token, refresh_token: token.refresh_token, expires_at: now.saturating_add(token.expires_in) })
    }
}

impl PendingGoogleLogin {
    pub fn authorization_url(&self) -> Url {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        let mut url = self.oauth.authorization.clone();
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.oauth.config.client_id.as_str()),
            ("redirect_uri", self.redirect.as_str()),
            ("response_type", "code"),
            ("scope", "openid email https://www.googleapis.com/auth/drive"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", &self.state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        url
    }

    /// Validate locally before making the one-time token exchange. The host
    /// should keep listening after unrelated loopback requests such as favicon.
    pub fn callback_code(&self, callback: &Url) -> Result<String, CloudError> {
        if callback.origin() != self.redirect.origin() || callback.path() != self.redirect.path() || callback.fragment().is_some() {
            return Err(CloudError::InvalidRequest("Google sign-in callback does not match this login".into()));
        }
        let pairs: Vec<_> = callback.query_pairs().collect();
        let states = pairs.iter().filter(|(key, _)| key == "state").map(|(_, value)| value.as_ref()).collect::<Vec<_>>();
        if states != [self.state.as_str()] { return Err(CloudError::InvalidRequest("Google sign-in callback state does not match this login".into())); }
        if pairs.iter().any(|(key, _)| key == "error") { return Err(CloudError::PermissionDenied); }
        let codes = pairs.iter().filter(|(key, _)| key == "code").map(|(_, value)| value.as_ref()).collect::<Vec<_>>();
        match codes.as_slice() { [code] if !code.is_empty() => Ok((*code).to_owned()), _ => Err(CloudError::InvalidRequest("Google sign-in callback is missing its authorization code".into())) }
    }

    pub async fn finish(self, callback: Url, now: u64) -> Result<(GoogleTokens, GoogleAccount), CloudError> {
        let code = self.callback_code(&callback)?;
        let mut form = vec![
            ("grant_type", "authorization_code"), ("code", code.as_str()), ("client_id", self.oauth.config.client_id.as_str()),
            ("redirect_uri", self.redirect.as_str()), ("code_verifier", self.verifier.as_str()),
        ];
        if let Some(secret) = &self.oauth.config.client_secret { form.push(("client_secret", secret)); }
        let tokens = self.oauth.exchange(&form, now).await?;
        let account = self.oauth.account(&tokens).await?;
        Ok((tokens, account))
    }
}

fn random_value() -> Result<String, CloudError> {
    let mut random = [0; 32];
    getrandom::fill(&mut random).map_err(|_| CloudError::InvalidRequest("Could not obtain secure randomness for cloud sign-in".into()))?;
    Ok(URL_SAFE_NO_PAD.encode(random))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_uses_pkce_and_rejects_unrelated_or_duplicate_callback_parameters() {
        let oauth = GoogleOAuth::new(GoogleOAuthConfig { client_id: "desktop.apps.googleusercontent.com".into(), client_secret: None }).unwrap();
        let login = oauth.begin_login(Url::parse("http://127.0.0.1:12345/oauth/callback").unwrap()).unwrap();
        let url = login.authorization_url();
        assert!(!url.as_str().contains(&login.verifier));
        assert!(url.query_pairs().any(|(key, value)| key == "code_challenge_method" && value == "S256"));
        let callback = Url::parse(&format!("http://127.0.0.1:12345/oauth/callback?state={}&code=code", login.state)).unwrap();
        assert_eq!(login.callback_code(&callback).unwrap(), "code");
        let mut wrong = callback.clone();
        wrong.set_port(Some(12346)).unwrap();
        assert!(login.callback_code(&wrong).is_err());
        let mut duplicate = callback;
        duplicate.query_pairs_mut().append_pair("state", &login.state);
        assert!(login.callback_code(&duplicate).is_err());
        let second = oauth.begin_login(Url::parse("http://127.0.0.1:12345/oauth/callback").unwrap()).unwrap();
        assert_ne!(login.state, second.state);
        assert_ne!(login.verifier, second.verifier);
        assert!(oauth.begin_login(Url::parse("http://example.com/callback").unwrap()).is_err());
    }
}
