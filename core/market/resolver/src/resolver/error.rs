use std::error;
use std::fmt;

// #region ParseError

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    msg: String,
}

impl ParseError {
    pub fn new(message: &str) -> Self {
        ParseError {
            msg: String::from(message),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for ParseError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region ResolveError

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    pub msg: String,
}

impl ResolveError {
    pub fn new(message: &str) -> Self {
        ResolveError {
            msg: String::from(message),
        }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for ResolveError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region ExpressionError

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionError {
    msg: String,
}

impl ExpressionError {
    pub fn new(message: &str) -> Self {
        ExpressionError {
            msg: String::from(message),
        }
    }
}

impl fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for ExpressionError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region PrepareError

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareError {
    msg: String,
}

impl PrepareError {
    pub fn new(message: &str) -> Self {
        PrepareError {
            msg: String::from(message),
        }
    }
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for PrepareError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region MatchError

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchError {
    msg: String,
}

impl MatchError {
    pub fn new(message: &str) -> Self {
        MatchError {
            msg: String::from(message),
        }
    }
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for MatchError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion
