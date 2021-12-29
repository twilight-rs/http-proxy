use http::{uri::InvalidUri, Method};
use hyper::Error as HyperError;
use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};
use twilight_http_ratelimiting::request::PathParseError;

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
