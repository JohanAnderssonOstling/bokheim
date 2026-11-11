//! Portable failures retain their classification when concrete platform errors cannot travel.
use std::{error::Error, fmt, io};

use serde::{Deserialize, Serialize};

use crate::{Classified, Failure};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Representation")]
pub struct OperationError {
    message: String,
    failure: Option<Failure>,
}

// Existing persisted update records contain diagnostic strings.
#[derive(Deserialize)]
#[serde(untagged)]
enum Representation {
    Classified { message: String, #[serde(default)] failure: Option<Failure> },
    Message(String),
}

impl From<Representation> for OperationError {
    fn from(value: Representation) -> Self {
        match value {
            Representation::Classified { message, failure } => Self { message, failure },
            Representation::Message(message) => message.into(),
        }
    }
}

impl OperationError {
    pub fn new(message: impl fmt::Display) -> Self {
        Self { message: message.to_string(), failure: None }
    }

    pub fn classified(failure: Failure, message: impl fmt::Display) -> Self {
        Self { message: message.to_string(), failure: Some(failure) }
    }

    pub fn operation(error: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        let error = error.into();
        Self::from_error(error.as_ref())
    }

    pub fn from_error(error: &(dyn Error + 'static)) -> Self {
        Self { message: error.to_string(), failure: Failure::from_error(error) }
    }

    pub fn from_classified(error: &(impl Classified + ?Sized)) -> Self {
        Self { message: error.to_string(), failure: error.failure() }
    }

    pub fn context(mut self, context: impl fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }
}

impl Classified for OperationError {
    fn failure(&self) -> Option<Failure> { self.failure }
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { formatter.write_str(&self.message) }
}

impl Error for OperationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> { self.failure.as_ref().map(|failure| failure as _) }
}

impl From<String> for OperationError {
    fn from(message: String) -> Self { Self { message, failure: None } }
}

impl From<&str> for OperationError {
    fn from(message: &str) -> Self { message.to_owned().into() }
}

impl From<io::Error> for OperationError {
    fn from(error: io::Error) -> Self { Self::from_error(&error) }
}

impl From<Failure> for OperationError {
    fn from(failure: Failure) -> Self { Self::classified(failure, failure) }
}

impl From<Box<dyn Error + Send + Sync>> for OperationError {
    fn from(error: Box<dyn Error + Send + Sync>) -> Self { Self::from_error(error.as_ref()) }
}

#[cfg(feature = "http")]
impl From<reqwest::Error> for OperationError {
    fn from(error: reqwest::Error) -> Self { Self::from_error(&error.without_url()) }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for OperationError {
    fn from(error: rusqlite::Error) -> Self { Self::from_error(&error) }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl From<wasm_bindgen::JsValue> for OperationError {
    fn from(error: wasm_bindgen::JsValue) -> Self {
        let property = |name| js_sys::Reflect::get(&error, &wasm_bindgen::JsValue::from_str(name)).ok().and_then(|value| value.as_string());
        let failure = js_sys::Reflect::get(&error, &"failure".into()).ok()
            .and_then(|value| js_sys::JSON::stringify(&value).ok()?.as_string())
            .and_then(|value| serde_json::from_str::<Option<Failure>>(&value).ok()).flatten()
            .or_else(|| property("name").as_deref().and_then(Failure::from_dom_name));
        let message = match (property("name"), property("message")) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (_, Some(message)) => message,
            _ => error.as_string().unwrap_or_else(|| format!("{error:?}")),
        };
        Self { message, failure }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl OperationError {
    pub fn into_js(self) -> wasm_bindgen::JsValue {
        let error = js_sys::Error::new(&self.message);
        if let Some(failure) = self.failure {
            if let Ok(value) = serde_json::to_string(&failure) {
                if let Ok(value) = js_sys::JSON::parse(&value) {
                    let _ = js_sys::Reflect::set(&error, &"failure".into(), &value);
                }
            }
        }
        error.into()
    }
}

impl From<Box<dyn Error>> for OperationError {
    fn from(error: Box<dyn Error>) -> Self { Self::from_error(error.as_ref()) }
}
