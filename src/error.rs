use rmp_serde::{decode, encode};
use std::fmt;
use std::io;
use std::str::Utf8Error;
use tokio_util::codec::LinesCodecError;

#[derive(Debug)]
pub enum Error {
    Error(String),
    MPDecodeError(decode::Error),
    MPEncodeError(encode::Error),
    IOError(io::Error),
    PasswordError(String),
    CodecError(LinesCodecError),
    Utf8Error(Utf8Error),
    Base64DecodeError(base64::DecodeError),
    ProtocolError(String),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Error(msg) => write!(f, "{}", msg),
            Error::MPDecodeError(error) => write!(f, "Error decoding data: {}", error),
            Error::MPEncodeError(error) => write!(f, "Error encoding data: {}", error),
            Error::IOError(error) => write!(f, "{}", error),
            Error::PasswordError(msg) => write!(f, "{}", msg),
            Error::CodecError(error) => write!(f, "{}", error),
            Error::Utf8Error(error) => write!(f, "{}", error),
            Error::Base64DecodeError(error) => write!(f, "{}", error),
            Error::ProtocolError(error) => write!(f, "{}", error),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<decode::Error> for Error {
    fn from(error: decode::Error) -> Self {
        Error::MPDecodeError(error)
    }
}

impl From<encode::Error> for Error {
    fn from(error: encode::Error) -> Self {
        Error::MPEncodeError(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::IOError(error)
    }
}

impl From<LinesCodecError> for Error {
    fn from(error: LinesCodecError) -> Self {
        Error::CodecError(error)
    }
}

impl From<Utf8Error> for Error {
    fn from(error: Utf8Error) -> Self {
        Error::Utf8Error(error)
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(error: std::net::AddrParseError) -> Self {
        Error::Error(format!("Error parsing host: {}", error))
    }
}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Error::Base64DecodeError(error)
    }
}
