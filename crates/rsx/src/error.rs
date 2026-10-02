use core::fmt;
use std::io;

/// The error type produced while rendering a template.
pub enum Error {
    /// The sink a [`Writer`](crate::Writer) streams into failed.
    Io(io::Error),
    /// A template returned an application error.
    Custom(Box<dyn std::error::Error + Send + Sync>),
}

/// Result alias used throughout generated code.
pub type Result<T = ()> = core::result::Result<T, Error>;

impl Error {
    /// Wraps an arbitrary error so a template body can bail out with `?`.
    pub fn custom<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Error::Custom(err.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "rsx: write failed: {e}"),
            Error::Custom(e) => write!(f, "rsx: {e}"),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => f.debug_tuple("Io").field(e).finish(),
            Error::Custom(e) => f.debug_tuple("Custom").field(&e.to_string()).finish(),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Custom(e) => Some(&**e),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => e,
            Error::Custom(e) => io::Error::other(e),
        }
    }
}
