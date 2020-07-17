use percent_encoding::percent_decode;

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
