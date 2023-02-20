use itertools::Itertools;
use std::collections::BTreeMap;
use std::convert::TryFrom;

use md5::{Digest, Md5};

use openssl::x509::{X509Ref, X509};

use crate::keystore::x509::PermissionsManager;
use crate::policy::CertPermissions;

/// Tries do decode base64. On failure tries to unescape snailquotes.
pub fn decode_data<S: AsRef<str>>(input: S) -> Result<Vec<u8>, DecodingError> {
    let no_whitespace: String = input.as_ref().split_whitespace().collect();
    match base64::decode(no_whitespace) {
        Ok(data) => Ok(data),
        Err(_) => Ok(snailquote::unescape(input.as_ref()).map(String::into_bytes)?),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodingError {
    #[error("invalid input base64: {0}")]
    BlobBase64(#[from] base64::DecodeError),
    #[error("invalid escaped json string: {0}")]
    BlobJsonString(#[from] snailquote::UnescapeError),
}

pub fn to_cert_data(
    certs: &Vec<X509>,
    permissions: &PermissionsManager,
) -> anyhow::Result<Vec<CertBasicData>> {
    let mut certs_data = Vec::new();
    for cert in certs {
        let data = CertBasicData::create(cert.as_ref(), permissions)?;
        certs_data.push(data);
    }
    Ok(certs_data)
}

pub struct CertBasicData {
    pub id: String,
    pub not_after: String,
    pub subject: BTreeMap<String, String>,
    pub permissions: String,
}

impl CertBasicData {
    pub fn create(cert: &X509Ref, permissions: &PermissionsManager) -> anyhow::Result<Self> {
        let mut data = CertBasicData::try_from(cert)?;
        let permissions = permissions.get(cert);
        data.permissions = format_permissions(&permissions);
        Ok(data)
    }
}

pub fn format_permissions(permissions: &Vec<CertPermissions>) -> String {
    if permissions.is_empty() {
        "none".to_string()
    } else {
        format!("{}", permissions.iter().format("|"))
    }
}

pub trait CertBasicDataVisitor {
    fn accept(&mut self, cert_data: CertBasicData);
}

/// Calculates Md5 of `txt` and returns first 8 characters.
pub fn str_to_short_hash(txt: impl AsRef<[u8]>) -> String {
    let digest = Md5::digest(txt);
    let digest = format!("{digest:x}");
    let short_hash = &digest[..8]; // Md5 is 32 characters
    short_hash.to_string()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    pub fn base64_wrapped_lines_test() {
        let wrapped_base64 = "
        VGhlIHF1aWNrIGJyb3du
        IGZveCBqdW1wcyBvdmVy
        IHRoZSBsYXp5IGRvZw==";
        let phrase = decode_data(wrapped_base64).expect("failed to decode base64 wrapped content");
        let phrase = String::from_utf8_lossy(&phrase).to_string();
        let expected = "The quick brown fox jumps over the lazy dog";
        assert_eq!(
            &phrase, expected,
            "Manifest related base64 payload may be encoded by the user, 
            and many tools wrap base64 output by default, 
            so we should try to filter out whitespace"
        )
    }
}
