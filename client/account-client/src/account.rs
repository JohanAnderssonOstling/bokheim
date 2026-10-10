use crate::session::{validate_email, validate_identity, AuthError, Session};
use account_contract::{AccountSnapshotResponse, MeResponse};
use reqwest::{header, Client};
use sync_common::api::libraries::{DeleteLibraryResponse, LibraryNameRequest, LibrarySummary};
use sync_common::transport as wire;

pub async fn current_account(session: &Session) -> Result<MeResponse, AuthError> {
    let response = Client::builder().build()?.get(session.server_url().join("api/auth/me")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    let account: MeResponse = wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?;
    validate_identity(&account.user_id, "user id")?;
    validate_email(&account.email)?;
    if account.user_id != session.user_id() || account.email != session.email() {
        return Err(AuthError::InvalidData("session identity does not match the identity authority".to_owned()));
    }
    Ok(account)
}

pub async fn account_snapshot(session: &Session) -> Result<AccountSnapshotResponse, AuthError> {
    let response = Client::builder().build()?.get(session.server_url().join("api/account/snapshot")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn list_account_sessions(session: &Session) -> Result<Vec<account_contract::SessionSummary>, AuthError> {
    let response = Client::builder().build()?.get(session.server_url().join("api/auth/sessions")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<account_contract::SessionRevocationResponse, AuthError> {
    validate_identity(session_id, "session id")?;
    let endpoint = format!("api/auth/sessions/{}", urlencoding::encode(session_id));
    let response = Client::builder().build()?.delete(session.server_url().join(&endpoint)?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<account_contract::OtherSessionsRevocationResponse, AuthError> {
    let response = Client::builder().build()?.delete(session.server_url().join("api/auth/sessions")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn create_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = LibraryNameRequest { library_id, library_name };
    let response = Client::builder()
        .build()?
        .post(session.server_url().join("api/libraries")?)
        .bearer_auth(session.token())
        .header(header::ACCEPT, wire::MEDIA_TYPE)
        .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
        .body(wire::encode(&request)?)
        .send()
        .await?
        .error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn rename_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = LibraryNameRequest { library_id, library_name };
    let response = Client::builder()
        .build()?
        .put(session.server_url().join("api/libraries")?)
        .bearer_auth(session.token())
        .header(header::ACCEPT, wire::MEDIA_TYPE)
        .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
        .body(wire::encode(&request)?)
        .send()
        .await?
        .error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn delete_library(session: &Session, library_id: &sync_common::LibraryId) -> Result<DeleteLibraryResponse, AuthError> {
    let response = Client::builder().build()?.delete(session.server_url().join(&format!("api/libraries/{library_id}"))?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn opds_feeds(session: &Session) -> Result<Vec<sync_common::api::opds_feeds::OpdsFeed>, AuthError> {
    let response = Client::builder().build()?.get(session.server_url().join("api/opds-feeds")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn put_opds_feed(session: &Session, feed: &sync_common::api::opds_feeds::OpdsFeed) -> Result<sync_common::api::opds_feeds::OpdsFeed, AuthError> {
    let response = Client::builder()
        .build()?
        .put(session.server_url().join("api/opds-feeds")?)
        .bearer_auth(session.token())
        .header(header::ACCEPT, wire::MEDIA_TYPE)
        .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
        .body(wire::encode(feed)?)
        .send()
        .await?
        .error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn delete_opds_feed(session: &Session, id: sync_common::api::opds_feeds::OpdsFeedId) -> Result<(), AuthError> {
    Client::builder().build()?.delete(session.server_url().join(&format!("api/opds-feeds/{id}"))?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(())
}

pub async fn cloud_storage(session: &Session) -> Result<Vec<sync_common::api::libraries::LibraryCloudStorage>, AuthError> {
    let response = Client::builder().build()?.get(session.server_url().join("api/cloud-storage")?).bearer_auth(session.token()).header(header::ACCEPT, wire::MEDIA_TYPE).send().await?.error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}

pub async fn set_cloud_storage(session: &Session, id: sync_common::LibraryId, change: sync_common::api::libraries::CloudStorageChange) -> Result<sync_common::api::libraries::LibraryCloudStorage, AuthError> {
    let response = Client::builder()
        .build()?
        .put(session.server_url().join(&format!("api/libraries/{id}/cloud-storage"))?)
        .bearer_auth(session.token())
        .header(header::ACCEPT, wire::MEDIA_TYPE)
        .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
        .body(wire::encode(&change)?)
        .send()
        .await?
        .error_for_status()?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}
