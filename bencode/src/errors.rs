use std::{num::ParseIntError, str::Utf8Error};

#[derive(Debug)]
pub enum ParseError {
    Int(ParseIntError),
    Str(Utf8Error),
    BadFile,
}

impl From<Utf8Error> for ParseError {
    fn from(error: Utf8Error) -> Self {
        ParseError::Str(error)
    }
}

impl From<ParseIntError> for ParseError {
    fn from(error: ParseIntError) -> Self {
        ParseError::Int(error)
    }
}
