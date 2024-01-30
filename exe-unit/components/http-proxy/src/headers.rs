use crate::http_to_gsb::HttpToGsbProxy;
use actix_http::header::HeaderMap;
use reqwest::RequestBuilder;
use std::collections::HashMap;

pub struct Headers {}

impl Headers {
    const IGNORED_HEADERS: [&'static str; 3] = ["host", "content-length", "connection"];

    pub fn filter(hm: &HeaderMap) -> HashMap<String, Vec<String>> {
        let mut headers = HashMap::new();

        for (name, value) in hm.iter() {
            log::info!("{} => {:?}", name, value);

            if !Headers::IGNORED_HEADERS.contains(&name.to_string().to_lowercase().as_str()) {
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

    pub fn add(
        mut builder: RequestBuilder,
        headers: HashMap<String, Vec<String>>,
    ) -> RequestBuilder {
        for (header_name, header_values) in headers {
            for value in header_values {
                builder = builder.header(header_name.clone(), value);
            }
        }
        builder
    }
}
