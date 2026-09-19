use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    InvalidInput(String),
    ParseError(String),
    InvalidSubcommand(String),
    MissingArgument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            Error::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Error::InvalidSubcommand(cmd) => {
                write!(f, "Invalid subcommand: '{}'. Use 'to-html' or 'to-md'", cmd)
            }
            Error::MissingArgument(arg) => write!(f, "Missing argument: {}", arg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
