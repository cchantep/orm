use std::error::Error as StdError;

use std::fmt;
use std::io::Error as IoError;
use std::str::Utf8Error;

use http::uri::InvalidUriParts;

#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: String) -> Error {
        Error { message: message }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        &self.message
    }
}

impl From<hyper::Error> for Error {
    fn from(hyperr: hyper::Error) -> Error {
        Error::new(format!("Hyper error: {}", hyperr))
    }
}

impl From<IoError> for Error {
    fn from(ioerr: IoError) -> Error {
        Error::new(format!("I/O error: {}", ioerr))
    }
}

impl From<Utf8Error> for Error {
    fn from(utf8_err: Utf8Error) -> Error {
        Error::new(format!("UTF8 error: {}", utf8_err))
    }
}

impl From<InvalidUriParts> for Error {
    fn from(invalid: InvalidUriParts) -> Error {
        Error::new(format!("Invalid URI: {}", invalid))
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(yamlerr: serde_yaml::Error) -> Error {
        Error::new(format!("YAML error: {}", yamlerr))
    }
}

/// Usage: `boxed_error!("Msg format: {}", details)`
#[macro_export]
macro_rules! boxed_error {
    ($fmt:expr $(, $values:expr )+) => (
        Err(std::boxed::Box::new(
            err::Error::new(format!($fmt, $($values),+))))?
    )
}

/// Usage: `new!("Msg format: {}", details)`
#[macro_export]
macro_rules! format_error {
    ($fmt:expr $(, $values:expr )+) => (
        err::Error::new(format!($fmt, $($values),+))
    )
}
