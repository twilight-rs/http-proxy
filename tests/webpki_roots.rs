//! Certificate verification with the same roots and crypto provider as the
//! proxy's `HttpsConnectorBuilder::with_webpki_roots` connector.
//!
//! The eMudhra chains are the ones served by the test hosts at
//! <https://emudhra.we-have-a-problem.com/> (from its `chains.zip`). Their
//! intermediates' private keys are compromised, so they must be rejected.

use rustls::{
    Error, RootCertStore,
    client::{WebPkiServerVerifier, danger::ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject},
};
use std::{sync::Arc, time::Duration};

/// 2026-11-06T00:00:00Z, within the validity of every fixture.
const NOW: Duration = Duration::from_secs(1_793_923_200);

fn verify(host: &'static str) -> Result<(), Error> {
    let path = format!("{}/tests/fixtures/{host}.pem", env!("CARGO_MANIFEST_DIR"));
    let chain = CertificateDer::pem_file_iter(path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (end_entity, intermediates) = chain.split_first().unwrap();

    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let verifier = WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .unwrap();

    verifier
        .verify_server_cert(
            end_entity,
            intermediates,
            &ServerName::try_from(host).unwrap(),
            &[],
            UnixTime::since_unix_epoch(NOW),
        )
        .map(|_| ())
}

#[test]
fn accepts_discord() {
    verify("discord.com").unwrap();
}

#[test]
fn rejects_compromised_emudhra_intermediates() {
    for host in [
        "dv-g3b-emudhra.we-have-a-problem.com",
        "dv-g3b-aaa-emudhra.we-have-a-problem.com",
        "dv-g3b-noct-emudhra.we-have-a-problem.com",
        "dv-g3b-aaa-noct-emudhra.we-have-a-problem.com",
    ] {
        let result = verify(host);
        assert!(
            matches!(result, Err(Error::InvalidCertificate(_))),
            "{host}: {result:?}"
        );
    }
}
