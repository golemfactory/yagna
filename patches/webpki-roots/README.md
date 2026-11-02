# Maintained CA roots for legacy TLS clients

Yagna's HTTP and DNS dependencies use both the `webpki-roots` 0.22 and 0.25
APIs. Their bundled CA stores contain the distrusted emSign ECC Root CA - G3
covered by [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381.html).

The workspace patches both versions with small API adapters. At build time,
`generate.rs` translates every trust anchor from the crates.io
`webpki-roots` **1.0.10 or newer**, preserving subjects, public keys, name
constraints, and ordering. The exact source version and checksum are pinned by
the workspace `Cargo.lock`. No legacy certificate bundle is retained.
The maintained bundle includes the upstream removal in
[webpki-roots PR #133](https://github.com/rustls/webpki-roots/pull/133).

The adapters preserve the legacy public representations, so existing
`awc`/`actix-tls`, `reqwest`, and `trust-dns` callers receive the updated roots
without a TLS or HTTP API migration. The source data is maintained upstream;
the adapters contain no independently maintained CA list. The upstream data
license is CDLA-Permissive-2.0.

## Validation

The adapter tests compare all generated fields with the maintained bundle and
check that the distrusted emSign G3 public key is absent. Run both adapters from
the workspace so tests use its pinned lockfile:

```sh
cargo test --locked -p ya-utils-networking --test tls_roots
```

## Audit scope

The original adapter version numbers are retained to satisfy dependency API
constraints. The local `cargo audit --no-fetch` run no longer reports
RUSTSEC-2026-0381 after these packages become path dependencies. This absence
alone is not evidence of a fix: verify the path patches and the actual generated
stores. Scanners matching only package names and versions may still flag the
adapters. No advisory is ignored, and no patched upstream version is claimed for
the legacy APIs. The lockfile currently pins the maintained bundle to 1.0.11.

This changes the bundled trust anchors. It does not backport the verifier's
public-key denylist from `rustls-webpki` 0.103.16, repair custom trust stores, or
resolve the other legacy TLS advisories. Remove these adapters once every
dependency uses a maintained roots API directly.
