use http::{Method, Response};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper_util::client::legacy::Error as HyperUtilError;
use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

static INVALID_METHOD_MSG: &str = "http-proxy: Unsupported HTTP method in request";
static REQUEST_ISSUE_MSG: &str = "http-proxy: Error requesting the Discord API";

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub enum RequestError {
    InvalidMethod { method: Method },
    RequestIssue { source: HyperUtilError },
}

impl RequestError {
    pub fn as_response(&self) -> Response<BoxBody<Bytes, hyper::Error>> {
        let (status_code, body_incoming) = match self {
            RequestError::InvalidMethod { .. } => (501, INVALID_METHOD_MSG),
            RequestError::RequestIssue { .. } => (502, REQUEST_ISSUE_MSG),
        };

        Response::builder()
            .status(status_code)
            .body(BoxBody::new(
                Full::from(body_incoming).map_err(|_| unreachable!()),
            ))
            .unwrap()
    }
}

impl Display for RequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidMethod { method } => {
                f.write_str("invalid method: ")?;
                method.fmt(f)
            }
            Self::RequestIssue { source } => {
                f.write_str("error executing request: ")?;
                source.fmt(f)
            }
        }
    }
}

impl Error for RequestError {}
