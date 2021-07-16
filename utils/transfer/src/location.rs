use std::path::PathBuf;

use percent_encoding::percent_decode;
use regex::Regex;
use url::{ParseError, Url};

use crate::error::Error;

pub trait UrlExt {
    fn path_decoded(&self) -> String;
}

impl UrlExt for url::Url {
    fn path_decoded(&self) -> String {
        percent_decode(self.path().as_bytes())
            .decode_utf8_lossy()
            .to_string()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferHash {
    pub alg: String,
    pub val: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferUrl {
    pub hash: Option<TransferHash>,
    pub url: Url,
}

impl TransferUrl {
    pub fn parse(url: &str, fallback_scheme: &str) -> Result<Self, Error> {
        let url = url.trim();
        if url.is_empty() {
            return Err(Error::InvalidUrlError("Empty URL".to_owned()));
        }

        let (hash, url) = parse_hash(url)?;
        let parsed = match Url::parse(url) {
            Ok(parsed_url) => match parsed_url.scheme().len() {
                // now this is dumb... Url::parse() will accept Windows absolute path, taking drive letter for scheme!
                #[cfg(windows)]
                1 => Url::parse(&format!("{}:{}", fallback_scheme, url))?,
                _ => parsed_url,
            },
            Err(error) => match error {
                ParseError::RelativeUrlWithoutBase => {
                    Url::parse(&format!("{}:{}", fallback_scheme, url))?
                }
                _ => return Err(Error::from(error)),
            },
        };

        Ok(TransferUrl { hash, url: parsed })
    }

    pub fn parse_with_hash(url: &str, fallback_scheme: &str) -> Result<Self, Error> {
        let parsed = Self::parse(url, fallback_scheme)?;
        match &parsed.hash {
            Some(_) => Ok(parsed),
            None => Err(Error::InvalidUrlError("Missing hash".to_owned())),
        }
    }

    pub fn file_name(&self) -> Result<String, Error> {
        let path = PathBuf::from(self.url.path_decoded());
        match path.file_name() {
            Some(name) => Ok(name.to_string_lossy().to_string()),
            None => Err(Error::InvalidUrlError(self.url.to_string())),
        }
    }
}

fn parse_hash(url: &str) -> Result<(Option<TransferHash>, &str), Error> {
    lazy_static::lazy_static! {
        static ref RE: Regex = Regex::new(r"(?i)hash:(//)?([^:]+):(0x)?([a-f0-9]+):(.+)").unwrap();
    }
    match RE.captures(url) {
        Some(captures) => {
            let hash = TransferHash {
                alg: captures.get(2).unwrap().as_str().to_owned(),
                val: hex::decode(captures.get(4).unwrap().as_str())?,
            };
            let url = captures.get(5).unwrap().as_str();
            Ok((Some(hash), url))
        }
        None => {
            if url.starts_with("hash:") {
                Err(Error::InvalidUrlError(url.to_owned()))
            } else {
                Ok((None, url))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::TransferUrl;

    macro_rules! should_fail {
        ($str:expr) => {
            assert!(
                TransferUrl::parse($str, "container").is_err(),
                "{} should fail",
                $str
            );
        };
    }

    macro_rules! should_succeed {
        ($str:expr) => {
            assert!(
                TransferUrl::parse($str, "container").is_ok(),
                "{} should succeed",
                $str
            );
        };
    }

    #[test]
    fn err() {
        should_fail!("");

        should_fail!("hash:");
        should_fail!("hash://");
        should_fail!("hash::");
        should_fail!("hash::val");
        should_fail!("hash://:val");
        should_fail!("hash::http://addr.com");
        should_fail!("hash://:http://addr.com");
        should_fail!("hash:alg");
        should_fail!("hash:alg:");
        should_fail!("hash:alg:val");
        should_fail!("hash:alg:0f0f0f0f0f");
        should_fail!("hash:alg:0f0f0f0f0f:");
        should_fail!("hash:alg:0x0f0f0f0f0f");
        should_fail!("hash:alg:0x0f0f0f0f0f:");
        should_fail!("hash:alg:val:");

        should_fail!("http:");
        should_fail!("http::");
        should_fail!("http://");
        should_fail!("http:://location.com");
        should_fail!("http:://");
        should_fail!("http::location.com");
    }

    #[test]
    fn ok() {
        should_succeed!("dir");
        should_succeed!("/dir");
        should_succeed!("dir/sub/file");
        should_succeed!("/dir/sub/file");
        should_succeed!("C:/");

        should_succeed!("/");
        should_succeed!("file:/");
        should_succeed!("file:/tmp/file");
        should_succeed!("file:///tmp/file");

        should_succeed!("hash:alg:ff00ff00:http://location.com");
        should_succeed!("hash:alg:0xff00ff00:http://location.com");
        should_succeed!("hash://alg:ff00ff00:http://location.com");
        should_succeed!("hash://alg:0xff00ff00:http://location.com");
        should_succeed!("HASH://alg:0xFF00FF00:http://location.com");

        should_succeed!("http://location.com");
        should_succeed!("http:location.com");
    }

    #[test]
    #[cfg(windows)]
    fn fallback_to_file_on_windows_path() {
        assert_eq!(
            TransferUrl::parse("C:\\Users", "file").unwrap(),
            TransferUrl {
                hash: None,
                url: url::Url::parse("file://C:/Users").unwrap()
            }
        );
    }
}
