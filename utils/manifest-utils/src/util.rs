use itertools::Itertools;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryFrom;
use std::ffi::OsStr;
use std::fs::{self, DirEntry, File};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use openssl::nid::Nid;
use openssl::x509::{X509Ref, X509};
use std::io::prelude::*;

use crate::policy::PermissionsManager;
use crate::Keystore;

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

pub fn parse_cert_file(cert: &PathBuf) -> anyhow::Result<Vec<X509>> {
    let extension = get_file_extension(cert);
    let mut cert = File::open(cert)?;
    let mut cert_buffer = Vec::new();
    cert.read_to_end(&mut cert_buffer)?;
    match extension {
        Some(ref der) if der.to_lowercase() == "der" => Ok(vec![X509::from_der(&cert_buffer)?]),
        Some(ref pem) if pem.to_lowercase() == "pem" => Ok(X509::stack_from_pem(&cert_buffer)?),
        _ => {
            // Certificates can have various other extensions like .cer .crt .key (and .key can be both DER and PEM)
            // Initial parsing dictated by `extension` is done because it would improper to parse .pem as a DER
            Ok(X509::stack_from_pem(&cert_buffer)
                .or_else(|_| X509::from_der(&cert_buffer).map(|cert| vec![cert]))?)
        }
    }
}

fn get_file_extension(path: &Path) -> Option<String> {
    path.extension().map(os_str_to_string)
}

fn get_file_name(path: &Path) -> Option<String> {
    path.file_name().map(os_str_to_string)
}

fn get_file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(os_str_to_string)
}

fn os_str_to_string(os_str: &OsStr) -> String {
    os_str.to_string_lossy().to_string()
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

/// Adds entries with given `nid` to given `subject` String.
fn add_cert_subject_entries(
    subject: &mut BTreeMap<String, String>,
    cert: &X509Ref,
    nid: Nid,
    entry_short_name: &str,
) {
    if let Some(entries) = cert_subject_entries(cert, nid) {
        subject.insert(entry_short_name.to_string(), entries);
    }
}

/// Reads subject entries and returns them as comma separated `String`.
fn cert_subject_entries(cert: &X509Ref, nid: Nid) -> Option<String> {
    let entries =
        cert.subject_name()
            .entries_by_nid(nid)
            .fold(String::from(""), |mut names, name| {
                if !names.is_empty() {
                    names.push_str(", ");
                }
                let name = String::from_utf8_lossy(name.data().as_slice());
                names.push_str(&name);
                names
            });
    if entries.is_empty() {
        return None;
    }
    Some(entries)
}

pub fn cert_to_id(cert: &X509Ref) -> anyhow::Result<String> {
    let txt = cert.to_text()?;
    Ok(str_to_short_hash(&txt))
}

pub fn visit_certificates<T: CertBasicDataVisitor>(
    cert_dir: &PathBuf,
    visitor: T,
) -> anyhow::Result<T> {
    let keystore = Keystore::load(cert_dir)?;
    let mut visitor = X509Visitor { visitor };
    keystore.visit_certs(&mut visitor)?;
    Ok(visitor.visitor)
}

pub struct CertBasicData {
    pub id: String,
    pub not_after: String,
    pub subject: BTreeMap<String, String>,
    pub permissions: String,
}

impl TryFrom<&X509Ref> for CertBasicData {
    type Error = anyhow::Error;

    fn try_from(cert: &X509Ref) -> Result<Self, Self::Error> {
        let id = cert_to_id(cert)?;
        let not_after = cert.not_after().to_string();
        let mut subject = BTreeMap::new();
        add_cert_subject_entries(&mut subject, cert, Nid::COMMONNAME, "CN");
        add_cert_subject_entries(&mut subject, cert, Nid::PKCS9_EMAILADDRESS, "E");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONNAME, "O");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONALUNITNAME, "OU");
        add_cert_subject_entries(&mut subject, cert, Nid::COUNTRYNAME, "C");
        add_cert_subject_entries(&mut subject, cert, Nid::STATEORPROVINCENAME, "ST");

        Ok(CertBasicData {
            id,
            not_after,
            subject,
            permissions: "".to_string(),
        })
    }
}

impl CertBasicData {
    pub fn create(cert: &X509Ref, permissions: &PermissionsManager) -> anyhow::Result<Self> {
        let mut data = CertBasicData::try_from(cert)?;
        let permissions = permissions.get(cert).unwrap_or(vec![]);
        data.permissions = format!("{}", permissions.iter().format("|"));
        Ok(data)
    }
}

pub trait CertBasicDataVisitor {
    fn accept(&mut self, cert_data: CertBasicData);
}

pub(crate) struct X509Visitor<T: CertBasicDataVisitor> {
    visitor: T,
}

impl<T: CertBasicDataVisitor> X509Visitor<T> {
    pub(crate) fn accept(
        &mut self,
        cert: &X509Ref,
        permissions: &PermissionsManager,
    ) -> anyhow::Result<()> {
        let cert_data = CertBasicData::create(cert, permissions)?;
        self.visitor.accept(cert_data);
        Ok(())
    }
}

pub struct KeystoreManager {
    keystore: Keystore,
    ids: HashSet<String>,
    cert_dir: PathBuf,
}

impl KeystoreManager {
    pub fn try_new(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        let keystore = Keystore::load(cert_dir)?;
        let ids = keystore.certs_ids()?;
        let cert_dir = cert_dir.clone();
        Ok(Self {
            ids,
            cert_dir,
            keystore,
        })
    }

    pub fn permissions_manager(&self) -> PermissionsManager {
        self.keystore.permissions_manager()
    }

    /// Copies certificates from given file to `cert-dir` and returns newly added certificates.
    /// Certificates already existing in `cert-dir` are skipped.
    pub fn load_certs(self, cert_paths: &Vec<PathBuf>) -> anyhow::Result<KeystoreLoadResult> {
        let mut loaded = HashMap::new();
        let mut skipped = HashMap::new();

        for cert_path in cert_paths {
            let mut new_certs = Vec::new();
            let file_certs = parse_cert_file(cert_path)?;
            if file_certs.is_empty() {
                continue;
            }
            let file_certs_len = file_certs.len();
            for file_cert in file_certs {
                let id = cert_to_id(&file_cert)?;
                if !self.ids.contains(&id) && !loaded.contains_key(&id) {
                    new_certs.push(file_cert.clone());
                    loaded.insert(id, file_cert);
                } else {
                    skipped.insert(id, file_cert);
                }
            }
            if file_certs_len == new_certs.len() {
                self.load_as_keychain_file(cert_path)?;
            } else {
                self.load_as_certificate_files(cert_path, new_certs)?;
            }
        }

        Ok(KeystoreLoadResult {
            loaded: loaded.into_values().collect(),
            skipped: skipped.into_values().collect(),
        })
    }

    pub fn remove_certs(self, ids: &HashSet<String>) -> anyhow::Result<KeystoreRemoveResult> {
        if ids.difference(&self.ids).eq(ids) {
            return Ok(KeystoreRemoveResult::NothingToRemove);
        }
        let mut removed = HashMap::new();

        let cert_dir_entries: Vec<Result<DirEntry, std::io::Error>> =
            std::fs::read_dir(self.cert_dir.clone())?.collect();
        for dir_entry in cert_dir_entries {
            let cert_file = dir_entry?;
            let cert_file = cert_file.path();
            let certs = parse_cert_file(&cert_file)?;
            if certs.is_empty() {
                // No certificates in parsed file.
                continue;
            }
            let mut ids_cert = certs.into_iter().fold(HashMap::new(), |mut certs, cert| {
                if let Ok(id) = cert_to_id(&cert) {
                    certs.insert(id, cert);
                }
                certs
            });
            let mut split_and_skip = false;
            for id in ids.iter() {
                if let Some(cert) = ids_cert.remove(id) {
                    removed.insert(id, cert);
                    split_and_skip = true;
                }
            }
            if split_and_skip {
                let file_stem = get_file_stem(&cert_file).expect("Cannot get file name stem");
                let dot_extension = get_file_extension(&cert_file)
                    .map_or_else(|| String::from(""), |ex| format!(".{ex}"));
                for (id, cert) in ids_cert {
                    let cert = cert.to_pem()?;
                    let mut file_path = self.cert_dir.clone();
                    let filename = format!("{file_stem}.{id}{dot_extension}");
                    file_path.push(filename);
                    fs::write(file_path, cert)?;
                }
                fs::remove_file(cert_file)?;
            }
        }

        let removed: Vec<X509> = removed.into_values().collect();
        Ok(KeystoreRemoveResult::Removed { removed })
    }

    /// Loads keychain file to `cert-dir`
    fn load_as_keychain_file(&self, cert_path: &PathBuf) -> anyhow::Result<()> {
        let file_name = get_file_name(cert_path)
            .ok_or_else(|| anyhow::anyhow!(format!("Cannot get filename of {cert_path:?}")))?;
        let mut new_cert_path = self.cert_dir.clone();
        new_cert_path.push(file_name);
        if new_cert_path.exists() {
            let file_stem = get_file_stem(&new_cert_path).expect("Has to have stem");
            let dot_extension = get_file_extension(&new_cert_path)
                .map(|ex| format!(".{ex}"))
                .unwrap_or_else(|| String::from(""));
            for i in 0..u32::MAX {
                let numbered_filename = format!("{file_stem}.{i}{dot_extension}");
                new_cert_path = self.cert_dir.clone();
                new_cert_path.push(numbered_filename);
                if !new_cert_path.exists() {
                    break;
                }
            }
            if new_cert_path.exists() {
                anyhow::bail!("Unable to load certificate");
            }
        }
        fs::copy(cert_path, new_cert_path)?;
        Ok(())
    }

    /// Loads certificates as individual files to `cert-dir`
    fn load_as_certificate_files(&self, cert_path: &Path, certs: Vec<X509>) -> anyhow::Result<()> {
        let file_stem = get_file_stem(cert_path)
            .ok_or_else(|| anyhow::anyhow!("Cannot get file name stem."))?;
        let dot_extension = get_file_extension(cert_path)
            .map(|ex| format!(".{ex}"))
            .unwrap_or_else(|| String::from(""));
        for cert in certs.into_iter() {
            let id = cert_to_id(&cert)?;
            let mut new_cert_path = self.cert_dir.clone();
            new_cert_path.push(format!("{file_stem}.{id}{dot_extension}"));
            let cert = cert.to_pem()?;
            fs::write(new_cert_path, cert)?;
        }
        Ok(())
    }
}

pub struct KeystoreLoadResult {
    pub loaded: Vec<X509>,
    pub skipped: Vec<X509>,
}

pub enum KeystoreRemoveResult {
    NothingToRemove,
    Removed { removed: Vec<X509> },
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
