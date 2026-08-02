//! Shared HTTP wire contracts used by both the client and server transports.
//!
//! These types deliberately contain no HTTP-framework or application logic.
//! Each boundary still authenticates, validates status codes, and maps errors
//! independently.

pub mod auth {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Eq)]
    pub struct LoginRequest {
        pub username: String,
        pub password: String,
    }

    #[derive(Serialize, Deserialize, PartialEq, Eq)]
    pub struct SignupRequest {
        pub username: String,
        pub password: String,
        pub invite: String,
    }

    #[derive(Serialize, Deserialize, PartialEq, Eq)]
    pub struct AuthResponse {
        pub token: String,
        /// Absolute Unix timestamp after which the bearer token is invalid.
        pub expires_at: i64,
        pub user_id: String,
        pub username: String,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct MeResponse {
        pub user_id: String,
        pub username: String,
    }
}

pub mod assets {
    use crate::ContentHash;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BlobsExistsRequest {
        pub hashes: Vec<ContentHash>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ThumbnailsExistsRequest {
        pub content_hashes: Vec<ContentHash>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ExistsResponse {
        pub present: Vec<ContentHash>,
    }
}

pub mod libraries {
    use crate::LibraryId;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct LibrarySummary {
        pub library_id: LibraryId,
        pub library_name: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
    pub struct UpsertLibraryRequest {
        pub library_id: LibraryId,
        pub library_name: String,
    }
}

#[cfg(test)]
mod tests {
    use super::{assets, auth, libraries};
    use crate::{ContentHash, LibraryId};
    use serde_json::json;

    fn hash(character: char) -> ContentHash {
        ContentHash::parse(&character.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn authentication_contracts_preserve_the_existing_wire_shape() {
        let login = auth::LoginRequest { username: "reader".into(), password: "secret".into() };
        assert_eq!(serde_json::to_value(login).unwrap(), json!({ "username": "reader", "password": "secret" }));

        let signup = auth::SignupRequest { username: "reader".into(), password: "secret".into(), invite: "invite-code".into() };
        assert_eq!(serde_json::to_value(signup).unwrap(), json!({ "username": "reader", "password": "secret", "invite": "invite-code" }));

        let response: auth::AuthResponse = serde_json::from_value(json!({
            "token": "token",
            "expires_at": 1_900_000_000,
            "user_id": "user-1",
            "username": "reader"
        }))
        .unwrap();
        assert_eq!(response.token, "token");
        assert_eq!(response.expires_at, 1_900_000_000);
        assert_eq!(response.user_id, "user-1");
        assert_eq!(response.username, "reader");
    }

    #[test]
    fn asset_contracts_keep_book_and_thumbnail_request_keys_distinct() {
        let book_hash = hash('a');
        let thumbnail_hash = hash('b');
        assert_eq!(serde_json::to_value(assets::BlobsExistsRequest { hashes: vec![book_hash.clone()] }).unwrap(), json!({ "hashes": [book_hash.as_str()] }));
        assert_eq!(serde_json::to_value(assets::ThumbnailsExistsRequest { content_hashes: vec![thumbnail_hash.clone()] }).unwrap(), json!({ "content_hashes": [thumbnail_hash.as_str()] }));

        let response: assets::ExistsResponse = serde_json::from_value(json!({ "present": [book_hash.as_str(), thumbnail_hash.as_str()] })).unwrap();
        assert_eq!(response.present, vec![book_hash, thumbnail_hash]);
    }

    #[test]
    fn library_catalogue_contract_preserves_stable_ids_and_names() {
        let request = libraries::UpsertLibraryRequest { library_id: LibraryId::parse("library-1").unwrap(), library_name: "History".to_string() };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value, json!({ "library_id": "library-1", "library_name": "History" }));
        assert_eq!(serde_json::from_value::<libraries::LibrarySummary>(value).unwrap(), libraries::LibrarySummary { library_id: request.library_id, library_name: request.library_name });
    }
}
