use super::{
    AddParams, AddResponse, Cert, CommonAddParams, Keystore, KeystoreBuilder, RemoveParams,
    RemoveResponse,
};
use anyhow::bail;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use golem_certificate::schemas::certificate::Fingerprint;
use openssl::{
    asn1::{Asn1Time, Asn1TimeRef},
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Public},
    sign::Verifier,
    x509::{
        store::{X509Store, X509StoreBuilder},
        X509ObjectRef, X509Ref, X509StoreContext, X509VerifyResult, X509,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write,
    fs::{self, DirEntry, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

pub const CERT_NAME: &str = "X.509";

#[derive(Eq, PartialEq)]
pub struct X509CertData {
    pub id: String,
    pub not_after: DateTime<Utc>,
    pub subject: BTreeMap<String, String>,
}

impl X509CertData {
    pub fn create(cert: &X509Ref) -> anyhow::Result<Self> {
        let id = cert_to_id(cert)?;
        let not_after = asn1_time_to_date_time(cert.not_after())?;
        let mut subject = BTreeMap::new();
        add_cert_subject_entries(&mut subject, cert, Nid::COMMONNAME, "CN");
        add_cert_subject_entries(&mut subject, cert, Nid::PKCS9_EMAILADDRESS, "E");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONNAME, "O");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONALUNITNAME, "OU");
        let data = X509CertData {
            id,
            not_after,
            subject,
        };
        Ok(data)
    }
}

fn asn1_time_to_date_time(time: &Asn1TimeRef) -> anyhow::Result<DateTime<Utc>> {
    // Openssl lib allows to access time only through ASN1_TIME_print.
    // Diff starting from epoch is a workaround to get `not_after` value.
    let time_diff = Asn1Time::from_unix(0)?.diff(time)?;
    let not_after = NaiveDateTime::from_timestamp_millis(0).unwrap()
        + Duration::days(time_diff.days as i64)
        + Duration::seconds(time_diff.secs as i64);
    Ok(DateTime::<Utc>::from_utc(not_after, Utc))
}

pub(super) struct AddX509Response {
    pub loaded: Vec<X509>,
    pub duplicated: Vec<X509>,
    pub invalid: Vec<PathBuf>,
    pub leaf_cert_ids: Vec<String>,
}

pub struct X509KeystoreBuilder {
    builder: X509StoreBuilder,
    cert_dir: PathBuf,
}

impl X509KeystoreBuilder {
    pub fn new(cert_dir: impl AsRef<Path>) -> Self {
        let builder = X509StoreBuilder::new().expect("OpenSSL works");
        let cert_dir = PathBuf::from(cert_dir.as_ref());
        Self { builder, cert_dir }
    }
}

impl KeystoreBuilder<X509KeystoreManager> for X509KeystoreBuilder {
    fn try_with(&mut self, file: &Path) -> anyhow::Result<()> {
        for cert in parse_cert_file(file)? {
            self.builder.add_cert(cert)?
        }
        Ok(())
    }

    fn build(self) -> anyhow::Result<X509KeystoreManager> {
        let keystore = self.builder.build();
        let inner = Arc::new(RwLock::new(CertStore::new(keystore)));
        let keystore = X509Keystore { store: inner };
        let ids = keystore.certs_ids()?;
        Ok(X509KeystoreManager {
            keystore,
            ids,
            cert_dir: self.cert_dir,
        })
    }
}

#[derive(Clone)]
pub(super) struct X509KeystoreManager {
    pub(super) keystore: X509Keystore,
    ids: HashSet<String>,
    cert_dir: PathBuf,
}

impl X509KeystoreManager {
    /// Copies certificates from given file to `cert-dir` and returns newly added certificates.
    /// Certificates already existing in `cert-dir` are skipped.
    fn add_certs<ADD: CommonAddParams>(&self, add: &ADD) -> anyhow::Result<AddX509Response> {
        let mut loaded = HashMap::new();
        let mut skipped = HashMap::new();
        let mut invalid = Vec::new();

        for cert_path in add.certs() {
            let mut new_certs = Vec::new();
            match parse_cert_file(cert_path) {
                Ok(file_certs) => {
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
                        super::copy_file(cert_path, &self.cert_dir)?;
                    } else {
                        // Splits certificate chain file into individual certificate files
                        // because some of the certificates are already loaded.
                        self.load_as_certificate_files(cert_path, new_certs)?;
                    }
                }
                Err(err) => {
                    log::warn!("Unable to parse X.509 certificate. Err: {err}");
                    invalid.push(cert_path.clone());
                }
            }
        }
        let loaded_leaf_cert_ids = leaf_certs(&loaded);
        let skipped_leaf_cert_ids = leaf_certs(&skipped);
        let leaf_cert_ids = loaded_leaf_cert_ids
            .into_iter()
            .chain(skipped_leaf_cert_ids.into_iter())
            .map(str::to_string)
            .collect();
        let loaded = loaded.into_values().collect();
        let duplicated = skipped.into_values().collect();
        Ok(AddX509Response {
            loaded,
            duplicated,
            invalid,
            leaf_cert_ids,
        })
    }

    /// Loads certificates as individual files to `cert-dir`
    fn load_as_certificate_files(&self, cert_path: &Path, certs: Vec<X509>) -> anyhow::Result<()> {
        let file_stem = super::get_file_stem(cert_path)
            .ok_or_else(|| anyhow::anyhow!("Cannot get file name stem."))?;
        let dot_extension = super::get_file_extension(cert_path)
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

fn leaf_certs(certs: &HashMap<String, X509>) -> Vec<&str> {
    if certs.len() == 1 {
        // when there is 1 cert it is a leaf cert
        return certs.keys().map(String::as_ref).collect();
    }
    certs
        .iter()
        .filter(|(_, cert)| {
            !certs
                .iter()
                .any(|(_, cert2)| cert.issued(cert2) == X509VerifyResult::OK)
        })
        .map(|(id, _)| id.as_str())
        .collect()
}

impl Keystore for X509KeystoreManager {
    fn reload(&self, cert_dir: &Path) -> anyhow::Result<()> {
        self.keystore.reload(cert_dir)
    }

    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse> {
        let AddX509Response {
            loaded,
            duplicated,
            invalid,
            leaf_cert_ids,
        } = self.add_certs(add)?;

        let added = loaded
            .into_iter()
            .map(|cert| X509CertData::create(&cert))
            .collect::<anyhow::Result<Vec<X509CertData>>>()?
            .into_iter()
            .map(Cert::X509)
            .collect();
        let duplicated = duplicated
            .into_iter()
            .map(|cert| X509CertData::create(&cert))
            .collect::<anyhow::Result<Vec<X509CertData>>>()?
            .into_iter()
            .map(Cert::X509)
            .collect();
        Ok(AddResponse {
            added,
            duplicated,
            invalid,
            leaf_cert_ids,
        })
    }

    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse> {
        let ids = &remove.ids;
        if ids.difference(&self.ids).eq(ids) {
            return Ok(RemoveResponse::default());
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
                    removed.insert(id.clone(), cert);
                    split_and_skip = true;
                }
            }
            if split_and_skip {
                let file_stem =
                    super::get_file_stem(&cert_file).expect("Cannot get file name stem");
                let dot_extension = super::get_file_extension(&cert_file)
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

        let removed: Vec<Cert> = removed
            .into_values()
            .map(|cert| X509CertData::create(&cert))
            .collect::<anyhow::Result<Vec<X509CertData>>>()?
            .into_iter()
            .map(Cert::X509)
            .collect();
        Ok(RemoveResponse { removed })
    }

    fn list(&self) -> Vec<Cert> {
        self.keystore.list().into_iter().map(Cert::X509).collect()
    }
}

struct CertStore {
    store: X509Store,
}

#[derive(Clone)]
pub struct X509Keystore {
    store: Arc<RwLock<CertStore>>,
}

impl Default for X509Keystore {
    fn default() -> Self {
        let store = X509StoreBuilder::new().expect("SSL works").build();
        let store = CertStore::new(store);
        Self {
            store: Arc::new(RwLock::new(store)),
        }
    }
}

impl CertStore {
    pub fn new(store: X509Store) -> CertStore {
        CertStore { store }
    }
}

impl X509Keystore {
    /// Reads DER or PEM certificates (or PEM certificate stacks) from `cert-dir` and creates new `X509Store`.
    pub fn load(cert_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut store = X509StoreBuilder::new()?;
        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let cert = dir_entry?.path();
            if let Err(e) = Self::load_file(&mut store, &cert) {
                log::debug!(
                    "Skipping '{}' while loading a X509Keystore. Error: {e}",
                    cert.display()
                );
            }
        }
        let store = CertStore::new(store.build());
        let store = Arc::new(RwLock::new(store));
        Ok(Self { store })
    }

    pub fn reload(&self, cert_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let keystore = X509Keystore::load(&cert_dir)?;
        self.replace(keystore);
        Ok(())
    }

    pub fn issuer(&self, cert: &X509Ref) -> anyhow::Result<Option<Fingerprint>> {
        let inner = self.store.read().unwrap();

        let res = inner
            .store
            .objects()
            .iter()
            .flat_map(X509ObjectRef::x509)
            .find(|candidate| candidate.issued(&cert) == X509VerifyResult::OK)
            .map(cert_to_id)
            .transpose()?;

        Ok(res)
    }

    fn replace(&self, other: X509Keystore) {
        let store = {
            let mut inner = other.store.write().unwrap();
            std::mem::replace(
                &mut (*inner),
                CertStore {
                    store: X509StoreBuilder::new().unwrap().build(),
                },
            )
        };
        let mut inner = self.store.write().unwrap();
        *inner = store;
    }

    fn list(&self) -> Vec<X509CertData> {
        let inner = self.store.read().unwrap();
        inner
            .store
            .objects()
            .iter()
            .flat_map(X509ObjectRef::x509)
            .map(X509CertData::create)
            .flat_map(|cert| match cert {
                Ok(cert) => Some(cert),
                Err(err) => {
                    log::debug!("Failed to read X509 cert. Err: {}", err);
                    None
                }
            })
            .collect()
    }

    /// Decodes byte64 `sig`, verifies `cert`and reads its pub key,
    /// prepares digest using `sig_alg`, verifies `data` using `sig` and pub key.
    pub fn verify_signature(
        &self,
        cert: impl AsRef<str>,
        sig: impl AsRef<str>,
        sig_alg: impl AsRef<str>,
        data: impl AsRef<str>,
    ) -> anyhow::Result<()> {
        let sig = crate::decode_data(sig)?;

        let pkey = self.verify_cert(cert)?;

        let msg_digest = MessageDigest::from_name(sig_alg.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Unknown signature algorithm: {}", sig_alg.as_ref()))?;
        let mut verifier = Verifier::new(msg_digest, pkey.as_ref())?;
        if !(verifier.verify_oneshot(&sig, data.as_ref().as_bytes())?) {
            return Err(anyhow::anyhow!("Invalid signature"));
        }
        Ok(())
    }

    pub fn certs_ids(&self) -> anyhow::Result<HashSet<String>> {
        let inner = self.store.read().unwrap();
        let mut ids = HashSet::new();
        for cert in inner.store.objects() {
            if let Some(cert) = cert.x509() {
                let id = cert_to_id(cert)?;
                ids.insert(id);
            }
        }
        Ok(ids)
    }

    fn load_file(store: &mut X509StoreBuilder, cert: &Path) -> anyhow::Result<()> {
        for cert in parse_cert_file(cert)? {
            store.add_cert(cert)?
        }
        Ok(())
    }

    fn verify_cert<S: AsRef<str>>(&self, cert: S) -> anyhow::Result<PKey<Public>> {
        let cert_chain = Self::decode_cert_chain(cert)?;
        let store = self
            .store
            .read()
            .map_err(|err| anyhow::anyhow!("Err: {}", err.to_string()))?;
        let cert = match cert_chain.last().map(Clone::clone) {
            Some(cert) => cert,
            None => bail!("Unable to verify certificate. No certificate."),
        };
        let mut cert_stack = openssl::stack::Stack::new()?;
        for cert in cert_chain {
            cert_stack.push(cert).unwrap();
        }
        let mut ctx = X509StoreContext::new()?;
        if !(ctx.init(&store.store, &cert, &cert_stack, |ctx| ctx.verify_cert())?) {
            bail!("Invalid certificate");
        }
        Ok(cert.public_key()?)
    }

    /// Decodes certificate chain.
    ///
    /// The certificates are sorted from leaf to root.
    pub fn decode_cert_chain<S: AsRef<str>>(cert: S) -> anyhow::Result<Vec<X509>> {
        let cert = crate::decode_data(cert)?;
        Ok(match X509::from_der(&cert) {
            Ok(cert) => vec![cert],
            Err(_) => X509::stack_from_pem(&cert)?,
        })
    }
}

fn parse_cert_file(cert: &Path) -> anyhow::Result<Vec<X509>> {
    let extension = super::get_file_extension(cert);
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

pub fn cert_to_id(cert: &X509Ref) -> anyhow::Result<String> {
    let bytes = cert.digest(MessageDigest::sha512())?;
    let mut digest = String::with_capacity(bytes.len() * 2);
    for byte in bytes.iter() {
        write!(digest, "{byte:02x}")?;
    }

    Ok(digest)
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

impl std::fmt::Debug for X509Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keystore")
    }
}

#[cfg(test)]
mod tests {
    use openssl::asn1::Asn1Time;
    use test_case::test_case;

    use super::asn1_time_to_date_time;

    // No test for malformed date because 'Asn1Time' arrvies from parsed certificate.
    #[test_case("20230329115959Z", "2023-03-29T11:59:59Z" ; "After epoch")]
    #[test_case("19000101000000Z", "1900-01-01T00:00:00Z" ; "Before epoch")]
    pub fn read_not_after_test(asn1_time: &str, expected_time: &str) {
        let asn1_time = Asn1Time::from_str(asn1_time).unwrap();
        let date_time = asn1_time_to_date_time(&asn1_time).unwrap();
        assert_eq!(
            expected_time,
            date_time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
    }
}
