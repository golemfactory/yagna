use std::{env, fmt::Write, fs, path::PathBuf};

/// Translate the maintained bundle into the borrowed-byte APIs used by old TLS clients.
/// This changes only the representation, retaining every trust anchor and constraint.
fn generate(legacy_webpki: bool) {
    println!("cargo:rerun-if-changed=../generate.rs");

    let mut output = String::from("// Generated from webpki-roots-current; do not edit.\n");
    if legacy_webpki {
        output.push_str(
            "pub static TLS_SERVER_ROOTS: webpki::TlsServerTrustAnchors<'static> = \
             webpki::TlsServerTrustAnchors(&[\n",
        );
    } else {
        output.push_str("pub const TLS_SERVER_ROOTS: &[TrustAnchor<'static>] = &[\n");
    }

    for anchor in webpki_roots_current::TLS_SERVER_ROOTS {
        let anchor_type = if legacy_webpki {
            "webpki::TrustAnchor"
        } else {
            "TrustAnchor"
        };
        writeln!(output, "{anchor_type} {{").unwrap();
        writeln!(output, "subject: &{:?},", anchor.subject.as_ref()).unwrap();
        writeln!(
            output,
            "spki: &{:?},",
            anchor.subject_public_key_info.as_ref()
        )
        .unwrap();
        match anchor.name_constraints.as_ref() {
            Some(constraints) => {
                writeln!(
                    output,
                    "name_constraints: Some(&{:?}),",
                    constraints.as_ref()
                )
                .unwrap();
            }
            None => output.push_str("name_constraints: None,\n"),
        }
        output.push_str("},\n");
    }
    output.push_str(if legacy_webpki { "]);\n" } else { "];\n" });

    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("roots.rs"),
        output,
    )
    .unwrap();
}
