use std::{io, path::PathBuf};

use thiserror::Error;

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by mesh loading, parsing, and rendering configuration.
#[derive(Debug, Error)]
pub enum Error {
    /// Filesystem error while reading a model or companion material file.
    #[error("failed to read {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Original I/O error.
        source: io::Error,
    },

    /// The file extension is not enabled or not supported.
    #[error(
        "unsupported mesh format for {path}; expected .obj or .stl with matching features enabled"
    )]
    UnsupportedFormat {
        /// Path that could not be loaded.
        path: PathBuf,
    },

    /// Text parser error with optional line number.
    #[error("parse error in {path}: {message}")]
    Parse {
        /// File being parsed.
        path: PathBuf,
        /// 1-based line number if known.
        line: Option<usize>,
        /// Human readable message.
        message: String,
    },

    /// Binary STL content was malformed.
    #[error("invalid binary STL in {path}: {message}")]
    InvalidBinaryStl {
        /// File being parsed.
        path: PathBuf,
        /// Human readable message.
        message: String,
    },

    /// A mesh has no usable geometry.
    #[error("mesh has no faces or vertices")]
    EmptyMesh,
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn parse(
        path: impl Into<PathBuf>,
        line: Option<usize>,
        message: impl Into<String>,
    ) -> Self {
        Self::Parse {
            path: path.into(),
            line,
            message: message.into(),
        }
    }
}
