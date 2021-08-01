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
                f.write_str("ChunkingRequest: ")?;
                source.fmt(f)
            }
            Self::DeserializeBody { source } => {
                f.write_str("DeserializeBody: ")?;
                source.fmt(f)
            }
            Self::InvalidPath { source } => {
                f.write_str("InvalidPath: ")?;
                source.fmt(f)
            }
            Self::InvalidMethod { method } => {
                f.write_str("InvalidMethod: ")?;
                method.fmt(f)
            }
            Self::NoPath { uri } => {
                f.write_str("NoPath: ")?;
                uri.fmt(f)
            }
            Self::ResponseAssembly { source } => {
                f.write_str("ResponseAssembly: ")?;
                source.fmt(f)
            }
            Self::RequestIssue { source } => {
                f.write_str("RequestIssue: ")?;
                source.fmt(f)
            }
        }
    }
}

impl Error for RequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ChunkingRequest { source } => Some(source),
            Self::DeserializeBody { source } => Some(source),
            Self::InvalidPath { source, .. } => Some(source),
            Self::InvalidMethod { .. } => None,
            Self::NoPath { .. } => None,
            Self::ResponseAssembly { source } => Some(source),
            Self::RequestIssue { source } => Some(source),
        }
    }
}
