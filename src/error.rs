use std::fmt;

#[derive(Debug)]
pub enum AppError {
    WebSocket(tokio_tungstenite::tungstenite::Error),
    Json(serde_json::Error),
    Url(url::ParseError),
    ChannelSend(String),
    Connection(String),
    Other(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebSocket(e) => write!(f, "WebSocket error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Url(e) => write!(f, "URL parse error: {e}"),
            Self::ChannelSend(msg) => write!(f, "Channel send error: {msg}"),
            Self::Connection(msg) => write!(f, "Connection error: {msg}"),
            Self::Other(msg) => write!(f, "Error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WebSocket(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Url(e) => Some(e),
            _ => None,
        }
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for AppError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<url::ParseError> for AppError {
    fn from(err: url::ParseError) -> Self {
        Self::Url(err)
    }
}
