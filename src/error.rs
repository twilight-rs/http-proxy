use http::{uri::InvalidUri, Method, Response};
use hyper::{Body, Error as HyperError};
use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};
use twilight_http_ratelimiting::request::PathParseError;

static ACQUIRING_TICKET_FAILED_MSG: &str =
    "http-proxy: Acquiring ticket from the ratelimiter failed";
static INVALID_URI_MSG: &str = "http-proxy: Failed to create URI for requesting Discord API";
static INVALID_METHOD_MSG: &str = "http-proxy: Unsupported HTTP method in request";
static INVALID_PATH_MSG: &str = "http-proxy: Failed to parse API path from client request";
static REQUEST_ISSUE_MSG: &str = "http-proxy: Error requesting the Discord API";

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub enum RequestError {
    AcquiringTicket {
        source: Box<dyn Error + Send + Sync>,
    },
    InvalidMethod {
        method: Method,
    },
    InvalidPath {
        source: PathParseError,
    },
    InvalidURI {
        source: InvalidUri,
    },
    RequestIssue {
        source: HyperError,
    },
}

impl RequestError {
    pub fn as_response(&self) -> Response<Body> {
        let (status_code, body) = match self {
            RequestError::AcquiringTicket { .. } => (500, ACQUIRING_TICKET_FAILED_MSG),
            RequestError::InvalidURI { .. } => (500, INVALID_URI_MSG),
            RequestError::InvalidMethod { .. } => (501, INVALID_METHOD_MSG),
            RequestError::InvalidPath { .. } => (501, INVALID_PATH_MSG),
            RequestError::RequestIssue { .. } => (502, REQUEST_ISSUE_MSG),
        };

        Response::builder()
            .status(status_code)
            .body(Body::from(body))
            .unwrap()
    }
}

impl Display for RequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::AcquiringTicket { source } => {
                f.write_str("error when acquiring ratelimiting ticket: ")?;
                source.fmt(f)
            }
            Self::InvalidMethod { method } => {
                f.write_str("invalid method: ")?;
                method.fmt(f)
            }
            Self::InvalidPath { source } => {
                f.write_str("invalid path: ")?;
                source.fmt(f)
            }
            Self::InvalidURI { source } => {
                f.write_str("generated uri for discord api is invalid: ")?;
                source.fmt(f)
            }
            Self::RequestIssue { source } => {
                f.write_str("error executing request: ")?;
                source.fmt(f)
            }
        }
    }
}

impl Error for RequestError {}
