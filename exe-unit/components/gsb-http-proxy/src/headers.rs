use actix_http::header::HeaderMap;
use reqwest::RequestBuilder;
use std::collections::{HashMap, HashSet};

pub struct Headers {
    ignored: HashSet<String>,
}

impl Headers {
    const IGNORED_HEADERS: [&'static str; 3] = ["host", "content-length", "connection"];

    pub fn default() -> Self {
        Headers {
            ignored: HashSet::from_iter(
                Headers::IGNORED_HEADERS.map(|s| s.to_string()).into_iter(),
            ),
        }
    }

    #[allow(dead_code)]
    pub fn new() -> Self {
        Headers {
            ignored: HashSet::new(),
        }
    }

    #[allow(dead_code)]
    pub fn ignore(&mut self, header: &str) -> &Self {
        self.ignored.insert(header.to_string());
        self
    }

    pub fn filter(&self, hm: &HeaderMap) -> HashMap<String, Vec<String>> {
        let mut headers = HashMap::new();

        for (name, value) in hm.iter() {
            log::debug!("{} => {:?}", name, value);

            if !self
                .ignored
                .contains(name.to_string().to_lowercase().as_str())
            {
                headers
                    .entry(name.to_string())
                    .and_modify(|e: &mut Vec<String>| {
                        e.push(value.to_str().unwrap_or_default().to_string())
                    })
                    .or_insert_with(|| vec![value.to_str().unwrap_or_default().to_string()]);
            }
        }
        headers
    }
}

pub fn add(mut builder: RequestBuilder, headers: HashMap<String, Vec<String>>) -> RequestBuilder {
    for (header_name, header_values) in headers {
        for value in header_values {
            builder = builder.header(header_name.clone(), value);
        }
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};
    use std::str::FromStr;

    fn mock_headers() -> HeaderMap {
        let mut result = HeaderMap::new();
        result.insert(
            HeaderName::from_str("header-1").unwrap(),
            HeaderValue::from_str("value-1").unwrap(),
        );
        result.insert(
            HeaderName::from_str("host").unwrap(),
            HeaderValue::from_str("127.128.129.130").unwrap(),
        );
        result
    }

    #[test]
    fn test_headers_new_not_filtered() {
        let hm = mock_headers();

        let result = Headers::new().filter(&hm);

        assert_eq!(
            result.into_keys().collect::<Vec<_>>(),
            vec!["header-1", "host"]
        );
    }

    #[test]
    fn test_headers_default_filtered() {
        let hm = mock_headers();

        let result = Headers::default().filter(&hm);

        assert_eq!(result.into_keys().collect::<Vec<_>>(), vec!["header-1"]);
    }

    #[test]
    fn test_headers_new_with_ignore_filtered() {
        let hm = mock_headers();

        let result = Headers::new().ignore("header-1").filter(&hm);

        assert_eq!(result.into_keys().collect::<Vec<_>>(), vec!["host"]);
    }
}
