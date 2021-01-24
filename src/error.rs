use http::{Error as HttpError, Uri};
use hyper::Error as HyperError;
use snafu::Snafu;
use twilight_http::{error::Error as TwilightError, routing::PathParseError};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum RequestError {
    ChunkingRequest { source: HyperError },
    InvalidPath { source: PathParseError },
    MakingResponseBody { source: HttpError },
    NoPath { uri: Uri },
    RequestIssue { source: TwilightError },
}
