use http::{Error as HttpError, Method, Uri};
use hyper::Error as HyperError;
use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};
use twilight_http::{
    error::Error as TwilightError, response::DeserializeBodyError, routing::PathParseError,
};

#[derive(Debug)]
pub enum RequestError {
    ChunkingRequest { source: HyperError },
    DeserializeBody { source: DeserializeBodyError },
    InvalidPath { source: PathParseError },
    InvalidMethod { method: Method },
    NoPath { uri: Uri },
    ResponseAssembly { source: HttpError },
    RequestIssue { source: TwilightError },
}

impl Display for RequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::ChunkingRequest { source } => {
                f.write_str("error when chunking request: ")?;
                source.fmt(f)
            }
            Self::DeserializeBody { source } => {
                f.write_str("failed to deserialize body: ")?;
                source.fmt(f)
            }
            Self::InvalidPath { source } => {
                f.write_str("invalid path: ")?;
                source.fmt(f)
            }
            Self::InvalidMethod { method } => {
                f.write_str("invalid method: ")?;
                method.fmt(f)
            }
            Self::NoPath { uri } => {
                f.write_str("no path in uri: ")?;
                uri.fmt(f)
            }
            Self::ResponseAssembly { source } => {
                f.write_str("error during response assembly: ")?;
                source.fmt(f)
            }
            Self::RequestIssue { source } => {
                f.write_str("error during request: ")?;
                source.fmt(f)
            }
        }
    }
}

impl Error for RequestError {}
