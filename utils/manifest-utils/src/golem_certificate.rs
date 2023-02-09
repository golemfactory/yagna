use url::Url;
use ya_client_model::NodeId;

pub struct GolemCertificate {
    pub node_id: NodeId,
    pub permissions: Vec<GolemPermission>,
    pub cert_ids_chain: Vec<CertificateId>,
}

impl GolemCertificate {
    fn new(id: &str, permissions: Vec<GolemPermission>) -> Self {
        Self {
            node_id: Default::default(),
            permissions,
            cert_ids_chain: vec![CertificateId::new(id)],
        }
    }
}

#[derive(Debug)]
pub struct CertificateId {
    pub public_key: String, // hex
    pub hash: String,       // hex
}

impl CertificateId {
    fn new(id: &str) -> Self {
        Self {
            public_key: format!("key {}", id),
            hash: format!("{id}"),
        }
    }
}

#[derive(Debug)]
pub enum VerificationError {
    InvalidData,
    Expired(CertificateId),
    InvalidSignature(CertificateId),
    PermissionsDoNotMatch(CertificateId), // the signer does not have all the required permissions
    UrlParseError(Vec<String>),
}

pub enum GolemPermission {
    All,
    ManifestOutboundUnrestricted,
    ManifestOutbound(Vec<Url>),
}

pub fn verify_golem_certificate(certificate: &str) -> Result<GolemCertificate, VerificationError> {
    match certificate {
        "all" => Ok(GolemCertificate::new(
            certificate,
            vec![GolemPermission::All],
        )),
        "outbound" => Ok(GolemCertificate::new(
            certificate,
            vec![GolemPermission::ManifestOutboundUnrestricted],
        )),
        "expired" => Err(VerificationError::Expired(CertificateId::new(certificate))),
        "invalid-signature" => Err(VerificationError::InvalidSignature(CertificateId::new(
            certificate,
        ))),
        "invalid-permissions" => Err(VerificationError::PermissionsDoNotMatch(
            CertificateId::new(certificate),
        )),
        "no-permissions" => Ok(GolemCertificate::new(certificate, vec![])),
        c if c.starts_with("outbound-urls") => {
            let mut parts = c.split('|');
            let id = parts.next().unwrap();
            let (urls, errors): (Vec<_>, Vec<_>) = parts
                .map(|s| Url::parse(s).map_err(|_| s.to_string()))
                .partition(Result::is_ok);
            if !errors.is_empty() {
                Err(VerificationError::UrlParseError(
                    errors.into_iter().map(Result::unwrap_err).collect(),
                ))
            } else {
                Ok(GolemCertificate::new(
                    id,
                    vec![GolemPermission::ManifestOutbound(
                        urls.into_iter().map(Result::unwrap).collect(),
                    )],
                ))
            }
        }
        _ => Err(VerificationError::InvalidData),
    }
}
