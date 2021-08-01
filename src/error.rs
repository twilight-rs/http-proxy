use http::{Method, Uri};
use hyper::Error as HyperError;
use std::{error::Error, fmt};
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
    ResponseAssembly { source: http::Error },
    RequestIssue { source: TwilightError },
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChunkingRequest { source } => {
                f.write_str("ChunkingRequest: ")?;
                f.write_str(&source.to_string())?;
            }
            Self::DeserializeBody { source } => {
                f.write_str("DeserializeBody: ")?;
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
            Self::ResponseAssembly { source } => {
                f.write_str("ResponseAssembly: ")?;
                f.write_str(&source.to_string())?;
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
            Self::DeserializeBody { source } => Some(source),
            Self::InvalidPath { source, .. } => Some(source),
            Self::InvalidMethod { .. } => None,
            Self::NoPath { .. } => None,
            Self::ResponseAssembly { source } => Some(source),
            Self::RequestIssue { source } => Some(source),
        }
    }
}
