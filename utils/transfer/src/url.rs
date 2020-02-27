use crate::error::Error;
use std::path::Path;
use url::{ParseError, Url};

#[derive(Clone, Debug)]
pub enum TransferLocation {
    Plain(Url),
    WithHash { url: Url, alg: String, val: Vec<u8> },
}

unsafe impl Send for TransferLocation {}

impl TransferLocation {
    pub fn parse(url: &str) -> Result<Self, Error> {
        match Url::parse(url.err_empty()?) {
            Ok(parsed) => match parsed.scheme() {
                "hash" => Self::parse_hash(parsed.path()),
                _ => Ok(TransferLocation::Plain(parsed)),
            },
            Err(error) => match error {
                ParseError::RelativeUrlWithoutBase => match Path::new(url).is_absolute() {
                    true => Self::parse(&format!("file:{}", url)),
                    false => Err(ParseError::RelativeUrlWithoutBase.into()),
                },
                _ => Err(error.into()),
            },
        }
    }

    pub fn url(&self) -> &Url {
        match self {
            TransferLocation::Plain(url) => url,
            TransferLocation::WithHash { url, .. } => url,
        }
    }

    fn parse_hash(url: &str) -> Result<Self, Error> {
        let mut split: Vec<String> = url.splitn(3, ':').map(|s| s.to_owned()).collect();
        if split.len() < 3 {
            return Err(Error::InvalidUrlError("Invalid segment count".to_owned()));
        }

        let url = Url::parse(&split.pop().unwrap().err_empty()?)?;
        let val = split
            .pop()
            .unwrap()
            .to_lowercase()
            .replacen("0x", "", 1)
            .err_empty()?;
        let val = hex::decode(val)?;
        let alg = split.pop().unwrap().err_empty()?.to_lowercase();

        Ok(TransferLocation::WithHash { url, alg, val })
    }
}

trait ErrEmpty<E>
where
    Self: Sized,
{
    fn err_empty(self) -> Result<Self, E>;
}

impl<A> ErrEmpty<Error> for A
where
    A: AsRef<str>,
{
    fn err_empty(self) -> Result<Self, Error> {
        if self.as_ref().len() == 0 {
            return Err(Error::InvalidUrlError("Empty segment".to_owned()));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod test {
    use super::TransferLocation;

    macro_rules! should_fail {
        ($str:expr) => {
            assert!(
                TransferLocation::parse($str).is_err(),
                format!("{} should fail", $str)
            );
        };
    }

    macro_rules! should_succeed {
        ($str:expr) => {
            assert!(
                TransferLocation::parse($str).is_ok(),
                format!("{} should succeed", $str)
            );
        };
    }

    #[test]
    fn err() {
        should_fail!("");
        should_fail!("arbitrary");

        should_fail!("hash");
        should_fail!("hash:");
        should_fail!("hash::");
        should_fail!("hash::val");
        should_fail!("hash::http://addr.com");
        should_fail!("hash:alg");
        should_fail!("hash:alg:");
        should_fail!("hash:alg:val");
        should_fail!("hash:alg:val:");

        should_fail!("http");
        should_fail!("http:");
        should_fail!("http::");
        should_fail!("http://");
        should_fail!("http:://location.com");
        should_fail!("http:://");
        should_fail!("http::location.com");
    }

    #[test]
    fn ok() {
        should_succeed!("/");
        should_succeed!("file:/");
        should_succeed!("file:/tmp/file");
        should_succeed!("file:///tmp/file");

        should_succeed!("hash:alg:ff00ff00:http://location.com");
        should_succeed!("hash:alg:0xff00ff00:http://location.com");

        should_succeed!("http://location.com");
        should_succeed!("http:location.com");
    }
}
