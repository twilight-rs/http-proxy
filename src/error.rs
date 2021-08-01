use http::{Method, Uri};
use hyper::Error as HyperError;
use std::{error::Error, fmt};
use twilight_http::{error::Error as TwilightError, routing::PathParseError};

#[derive(Debug)]
pub enum RequestError {
    ChunkingRequest { source: HyperError },
    InvalidPath { source: PathParseError },
    InvalidMethod { method: Method },
    NoPath { uri: Uri },
    RequestIssue { source: TwilightError },
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChunkingRequest { source } => {
                f.write_str("ChunkingRequest: ")?;
                f.write_str(&source.to_string())?;
            }
            Self::InvalidPath { source } => {
                f.write_str("InvalidPath: ")?;
                f.write_str(&source.to_string())?;
            }
            Self::InvalidMethod { method } => {
                f.write_str("InvalidMethod: ")?;
                f.write_str(&method.to_string())?;
            }
            Self::NoPath { uri } => {
                f.write_str("InvalidMethod: ")?;
                f.write_str(&uri.to_string())?;
            }
            Self::RequestIssue { source } => {
                f.write_str("RequestIssue: ")?;
                f.write_str(&source.to_string())?;
            }
        }

        Ok(())
    }
}

impl Error for RequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ChunkingRequest { source } => Some(source),
            Self::InvalidPath { source, .. } => Some(source),
            Self::InvalidMethod { .. } => None,
            Self::NoPath { .. } => None,
            Self::RequestIssue { source } => Some(source),
        }
    }
}
