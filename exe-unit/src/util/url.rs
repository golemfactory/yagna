use crate::error::TransferError;
use regex::Regex;
use std::path::PathBuf;
use url::{ParseError, Url};
use ya_transfer::UrlExt;

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

unsafe impl Send for TransferUrl {}

impl TransferUrl {
    pub fn file_name(&self) -> Result<String, TransferError> {
        let path = PathBuf::from(self.url.path_decoded());
        match path.file_name() {
            Some(name) => Ok(name.to_string_lossy().to_string()),
            None => Err(TransferError::InvalidUrlError(self.url.to_string())),
        }
    }

    pub fn parse(url: &str, fallback_scheme: &str) -> Result<Self, TransferError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(TransferError::InvalidUrlError("Empty URL".to_owned()));
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
                _ => return Err(TransferError::from(error)),
            },
        };

        Ok(TransferUrl { hash, url: parsed })
    }

    pub fn parse_with_hash(url: &str, fallback_scheme: &str) -> Result<Self, TransferError> {
        let parsed = Self::parse(url, fallback_scheme)?;
        match &parsed.hash {
            Some(_) => Ok(parsed),
            None => Err(TransferError::InvalidUrlError("Missing hash".to_owned())),
        }
    }

    pub fn map_scheme<F>(mut self, f: F) -> Result<Self, TransferError>
    where
        F: Fn(&str) -> &str,
    {
        let scheme = self.url.scheme().to_owned();
        let new_scheme = f(&scheme);
        self.url = Url::parse(&self.url.as_str().replacen(&scheme, new_scheme, 1))?;
        Ok(self)
    }

    pub fn map_path<F>(mut self, f: F) -> Result<Self, TransferError>
    where
        F: FnOnce(&str, &str) -> Result<String, TransferError>,
    {
        let new_path = f(self.url.scheme(), self.url.path_decoded().as_str())?;
        self.url.set_path(&new_path);
        Ok(self)
    }
}

fn parse_hash(url: &str) -> Result<(Option<TransferHash>, &str), TransferError> {
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
                Err(TransferError::InvalidUrlError(url.to_owned()))
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
