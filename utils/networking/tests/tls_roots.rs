// Exercise both legacy APIs through the workspace patches and pinned lockfile.
type Anchor = (&'static [u8], &'static [u8], Option<&'static [u8]>);

fn stores() -> [Vec<Anchor>; 2] {
    [
        webpki_roots_022::TLS_SERVER_ROOTS
            .0
            .iter()
            .map(|a| (a.subject, a.spki, a.name_constraints))
            .collect(),
        webpki_roots_025::TLS_SERVER_ROOTS
            .iter()
            .map(|a| (a.subject, a.spki, a.name_constraints))
            .collect(),
    ]
}

use sha2::{Digest, Sha256};

#[test]
fn trust_store_matches_the_maintained_bundle_without_losing_constraints() {
    let expected: Vec<_> = webpki_roots_current::TLS_SERVER_ROOTS
        .iter()
        .map(|anchor| {
            (
                anchor.subject.as_ref(),
                anchor.subject_public_key_info.as_ref(),
                anchor.name_constraints.as_ref().map(|value| value.as_ref()),
            )
        })
        .collect();
    for actual in stores() {
        assert!(!actual.is_empty());
        assert_eq!(actual, expected);
    }
}

#[test]
fn distrusted_emsign_g3_key_is_absent_even_if_the_subject_changes() {
    // SHA-256 of the SPKI sequence contents in both original legacy bundles.
    // The upstream PR quotes the digest of the complete DER SPKI sequence instead.
    // https://github.com/rustls/webpki-roots/pull/133
    const DISTRUSTED_SPKI: &str =
        "e5004cd7da050648e22344ab697f6975ad6a5a0c005428f1d6fca14e92c7582d";
    for (_, spki, _) in stores().into_iter().flatten() {
        assert_ne!(format!("{:x}", Sha256::digest(spki)), DISTRUSTED_SPKI);
    }
}
