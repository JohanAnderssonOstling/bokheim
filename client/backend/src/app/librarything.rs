use book_metadata::librarything::LibraryThingClient;

const LIBRARYTHING_TOKEN_ENV: &str = "BOKHEIM_LIBRARYTHING_TOKEN";

pub(super) fn client_from_environment() -> Result<Option<LibraryThingClient>, book_metadata::external::ExternalMetadataError> {
    let Some(token) = std::env::var(LIBRARYTHING_TOKEN_ENV).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    LibraryThingClient::new(token).map(Some)
}
