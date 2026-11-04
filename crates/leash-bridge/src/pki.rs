//! A throw-away certificate authority and two leaf identities, for the demo and the tests.
//!
//! In a real deployment the CA key lives with a person, the controller key only on the host and the agent key only in
//! the guest; this module exists so that `docker compose up` and `cargo test` can exercise the real handshake without
//! anyone creating files by hand. Nothing here is written to disk.

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// DNS name carried by the controller's certificate. The agent accepts exactly this identity.
pub const CONTROLLER_NAME: &str = "leash-controller";
/// DNS name carried by the agent's certificate. The controller dials exactly this identity.
pub const AGENT_NAME: &str = "leash-agent";

#[derive(Debug, thiserror::Error)]
#[error("certificate generation failed: {0}")]
pub struct PkiError(String);

impl From<rcgen::Error> for PkiError {
    fn from(e: rcgen::Error) -> Self {
        Self(e.to_string())
    }
}

/// A certificate with its private key.
pub struct Identity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Identity(..)")
    }
}

impl Identity {
    fn clone_parts(&self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        (self.chain.clone(), self.key.clone_key())
    }

    pub(crate) fn into_parts(self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        (self.chain, self.key)
    }

    /// A second handle on the same identity.
    #[must_use]
    pub fn duplicate(&self) -> Self {
        let (chain, key) = self.clone_parts();
        Self { chain, key }
    }
}

/// When a leaf is valid. The default is "now, for a day"; tests use a window in the past.
#[derive(Debug, Clone, Copy)]
pub enum Validity {
    Current,
    Expired,
}

/// The CA and the means to issue leaves from it.
pub struct Authority {
    ca_der: CertificateDer<'static>,
    issuer: Issuer<'static, KeyPair>,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Authority(..)")
    }
}

impl Authority {
    /// # Errors
    /// Key or certificate generation failed.
    pub fn new(common_name: &str) -> Result<Self, PkiError> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
            .distinguished_name
            .push(DnType::CommonName, common_name);
        let cert = params.self_signed(&key)?;
        Ok(Self {
            ca_der: cert.der().clone(),
            issuer: Issuer::new(params, key),
        })
    }

    /// The CA certificate, to be trusted by both ends.
    #[must_use]
    pub fn certificate(&self) -> CertificateDer<'static> {
        self.ca_der.clone()
    }

    /// Issue a leaf for `dns_name` that can act as a TLS server and as a TLS client.
    ///
    /// # Errors
    /// Key or certificate generation failed.
    pub fn issue(&self, dns_name: &str, validity: Validity) -> Result<Identity, PkiError> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![dns_name.to_owned()])?;
        params.distinguished_name.push(DnType::CommonName, dns_name);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        if matches!(validity, Validity::Expired) {
            params.not_before = rcgen::date_time_ymd(2020, 1, 1);
            params.not_after = rcgen::date_time_ymd(2020, 1, 2);
        }
        let cert = params.signed_by(&key, &self.issuer)?;
        Ok(Identity {
            chain: vec![cert.der().clone()],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        })
    }
}

/// DNS names in the certificate's subject alternative names.
pub(crate) fn dns_names(cert: &CertificateDer<'_>) -> Vec<String> {
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::{FromDer, X509Certificate};
    let Ok((_, parsed)) = X509Certificate::from_der(cert.as_ref()) else {
        return Vec::new();
    };
    let Ok(Some(san)) = parsed.subject_alternative_name() else {
        return Vec::new();
    };
    san.value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::DNSName(dns) => Some((*dns).to_owned()),
            _ => None,
        })
        .collect()
}
