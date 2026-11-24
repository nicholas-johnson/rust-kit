use std::fmt;

/// Errors a resolver, renderer or action can return. Each maps to a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No signed-in user. Pages redirect to the login route; the API returns 401.
    Unauthorized,
    Forbidden,
    NotFound,
    /// The request was understood but its input was invalid.
    BadRequest(String),
    /// An upstream service failed or returned something unusable.
    Upstream(String),
    Internal(String),
}

impl Error {
    pub fn internal(e: impl fmt::Display) -> Self {
        Error::Internal(e.to_string())
    }

    pub fn status(&self) -> u16 {
        match self {
            Error::Unauthorized => 401,
            Error::Forbidden => 403,
            Error::NotFound => 404,
            Error::BadRequest(_) => 400,
            Error::Upstream(_) => 502,
            Error::Internal(_) => 500,
        }
    }

    /// Text that is safe to show a visitor. Internal details stay in the log.
    pub fn public_message(&self) -> &str {
        match self {
            Error::Unauthorized => "Please sign in.",
            Error::Forbidden => "You don't have access to this.",
            Error::NotFound => "Not found.",
            Error::BadRequest(m) => m,
            Error::Upstream(_) => "A service this page depends on is unavailable. Please try again.",
            Error::Internal(_) => "Something went wrong.",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Upstream(m) => write!(f, "upstream error: {m}"),
            Error::Internal(m) => write!(f, "internal error: {m}"),
            other => f.write_str(other.public_message()),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::internal(e)
    }
}
