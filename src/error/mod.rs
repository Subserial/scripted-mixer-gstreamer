/*
 * Copyright 2022 Google LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
*/

pub type ParseResult<T> = std::result::Result<T, ParseError>;

#[derive(Debug, Clone)]
pub struct ParseError {
    err: String,
}

impl ParseError {
    pub fn report(s: &str) -> ParseError {
        ParseError{err: String::from(s)}
    }

    pub fn report_string(s: String) -> ParseError {
        ParseError{err: s}
    }

    pub fn annotate(&self, s: &str) -> ParseError {
        let mut err = String::from(s);
        err.push_str(": ");
        err.push_str(self.err.as_str());
        return ParseError { err, }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.err)
    }
}

impl std::error::Error for ParseError {}

impl From<&str> for ParseError {
    fn from(item: &str) -> ParseError {
        ParseError::report(item)
    }
}

impl From<String> for ParseError {
    fn from(item: String) -> ParseError {
        ParseError::report_string(item)
    }
}

#[macro_export]
macro_rules! catch_bail {
    ( $x:expr, $y:expr ) => {
        {
            match $x {
                Ok(val) => val,
                Err(_) => {
                    return Err($y.into());
                }
            }
        }
    };
}

#[macro_export]
macro_rules! catch_bail_annotate {
    ( $x:expr, $y:expr ) => {
        {
            match $x {
                Ok(val) => val,
                Err(err) => {
                    return Err(format!("{}: {}", $y, err).into());
                }
            }
        }
    };
}

#[macro_export]
macro_rules! none_bail {
    ( $x:expr, $y:expr ) => {
        {
            match $x {
                Some(val) => val,
                None => {
                    return Err($y.into());
                }
            }
        }
    };
}
