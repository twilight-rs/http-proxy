use dawn_http::{
    error::Error as DawnError,
    routing::PathParseError,
};
use http::{Error as HttpError, Uri};
use hyper::Error as HyperError;
use reqwest::Error as ReqwestError;
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum RequestError {
    ChunkingRequest {
        source: HyperError,
    },
    ChunkingResponse {
        source: ReqwestError,
    },
    InvalidPath {
        source: PathParseError,
    },
    MakingResponseBody {
        source: HttpError,
    },
    NoPath {
        uri: Uri,
    },
    RequestIssue {
        source: DawnError,
    },
}
