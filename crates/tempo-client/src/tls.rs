//! The LAN road's TLS, the remote PC's half: TLS 1.3 only, with raw public keys both ways (RFC
//! 7250), this computer's own key for the shack and the shack's key pinned ([`client`]). The
//! shack's half, and everything the shack refuses before a byte of application data is read, is
//! src-tauri's `remote_service::lan::tls`, which takes the one scheme, the key check and the
//! provider from here, so the two halves cannot drift apart. rustls with ring, both already
//! Nexus's, so nothing new is linked.
use std::sync::Arc;

use rustls::crypto::{verify_tls13_signature_with_raw_key, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, SubjectPublicKeyInfoDer, UnixTime};
use rustls::sign::CertifiedKey;
use rustls::{CertificateError, DigitallySignedStruct, Error, PeerIncompatible, SignatureScheme};
use tempo_stream::protocol;

/// The one signature scheme either end may use: ECDSA over P-256 with SHA-256, the keys the
/// stream already signs with (A5, S3-M1).
pub const SCHEME: SignatureScheme = SignatureScheme::ECDSA_NISTP256_SHA256;

/// Is this the SPKI of a P-256 key, exactly as a device key is (`protocol::device_key`)?
pub fn p256(spki: &[u8]) -> bool {
    protocol::hex_bytes(protocol::P256_SPKI_PREFIX_HEX)
        .is_some_and(|prefix| spki.len() == prefix.len() + 65 && spki.starts_with(&prefix))
}

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The remote PC's half ([`crate::road`]): TLS 1.3, its own raw key for this shack, and the
/// shack's key pinned. No resumption and no early data, as on the shack's side, so every
/// connection proves both keys again.
pub mod client {
    use super::*;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::client::{AlwaysResolvesClientRawPublicKeys, ClientConfig, Resumption};
    use rustls::pki_types::ServerName;

    /// The shack's key, pinned: anything else is refused before the client says a word (TLS 1.3
    /// proves the server's key before the client sends its own). `None` only for a pairing
    /// connection, which takes whatever P-256 key the shack presents and has the pairing's proofs
    /// bind it.
    #[derive(Debug)]
    struct PinnedShack {
        spki: Option<Vec<u8>>,
        provider: Arc<CryptoProvider>,
    }

    impl ServerCertVerifier for PinnedShack {
        fn verify_server_cert(
            &self,
            key: &CertificateDer<'_>,
            intermediates: &[CertificateDer<'_>],
            _name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            let pinned = match &self.spki {
                Some(spki) => key.as_ref() == spki.as_slice(),
                None => p256(key.as_ref()),
            };
            if intermediates.is_empty() && pinned {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(CertificateError::ApplicationVerificationFailure.into())
            }
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _key: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Err(PeerIncompatible::Tls12NotOfferedOrEnabled.into())
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            key: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            if dss.scheme != SCHEME {
                return Err(PeerIncompatible::NoSignatureSchemesInCommon.into());
            }
            verify_tls13_signature_with_raw_key(
                message,
                &SubjectPublicKeyInfoDer::from(key.as_ref()),
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![SCHEME]
        }

        fn requires_raw_public_keys(&self) -> bool {
            true
        }
    }

    /// A remote PC holding `key` (PKCS#8, lowercase hex), with the shack's `shack` key (SPKI,
    /// lowercase hex) pinned. `None` for a key or a pin that is not a P-256 one.
    pub fn config(key: &str, shack: &str) -> Option<Arc<ClientConfig>> {
        let pinned = protocol::hex_bytes(shack).filter(|spki| p256(spki))?;
        made(key, Some(pinned))
    }

    /// A remote PC holding `key`, pairing: no shack key pinned yet, so it takes the one presented,
    /// for this one connection only.
    pub fn pairing(key: &str) -> Option<Arc<ClientConfig>> {
        made(key, None)
    }

    fn made(key: &str, shack: Option<Vec<u8>>) -> Option<Arc<ClientConfig>> {
        let provider = provider();
        let signing = provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(protocol::hex_bytes(key)?.into()))
            .ok()?;
        let spki = signing.public_key()?.as_ref().to_vec();
        if !p256(&spki) {
            return None;
        }
        let certified = Arc::new(CertifiedKey::new(vec![CertificateDer::from(spki)], signing));
        let mut config = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .ok()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedShack {
                spki: shack,
                provider,
            }))
            .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(certified)));
        config.resumption = Resumption::disabled();
        config.enable_early_data = false;
        Some(Arc::new(config))
    }
}
