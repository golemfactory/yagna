use url::Url;

pub struct GolemCertificate {
    pub node_id: String,
    pub permissions: Vec<GolemPermission>,
    pub root_certificate_id: CertificateId,
}

impl GolemCertificate {
    fn new(id: &str, permission: GolemPermission) -> Self {
        Self {
            node_id: id.to_string(),
            permissions: vec![permission],
            root_certificate_id: CertificateId::new(id),
        }
    }
}

pub struct CertificateId {
    pub public_key: String, // hex
    pub hash: String, // hex
}

impl CertificateId {
    fn new(id: &str) -> Self {
        Self { public_key: format!("root key {}", id), hash: format!("root hash {}", id) }
    }
}

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
        "all" => Ok(GolemCertificate::new(certificate, GolemPermission::All)),
        "outbound" => Ok(GolemCertificate::new(certificate, GolemPermission::ManifestOutboundUnrestricted)),
        "expired" => Err(VerificationError::Expired(CertificateId::new(certificate))),
        "invalid-signature" => Err(VerificationError::InvalidSignature(CertificateId::new(certificate))),
        "invalid-permissions" => Err(VerificationError::PermissionsDoNotMatch(CertificateId::new(certificate))),
        c if c.starts_with("outbound-urls") => {
            let mut parts = c.split('|');
            let id = parts.next().unwrap();
            let (urls, errors): (Vec<_>, Vec<_>) = parts.map(|s| Url::parse(s).map_err(|_| s.to_string())).partition(Result::is_ok);
            if errors.len() > 0 {
                Err(VerificationError::UrlParseError(errors.into_iter().map(Result::unwrap_err).collect()))
            } else {
                Ok(GolemCertificate::new(id, GolemPermission::ManifestOutbound(urls.into_iter().map(Result::unwrap).collect())))
            }
        },
        _ => Err(VerificationError::InvalidData),
    }
}
