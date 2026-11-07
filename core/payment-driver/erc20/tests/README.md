# HTTP certificate verification

Yagna's reqwest clients, including the clients constructed internally by the
ERC20 RPC pool, use Rustls with the bundled Mozilla CA roots. This policy applies
to reqwest HTTPS. Other transports, including web3 WebSocket TLS and Unix awc
clients, still have separate TLS configurations.

Run these commands from the repository root.

## Check the production feature graph

```sh
python3 .github/scripts/check_reqwest_tls.py
```

CI checks Yagna, exe-unit and the ERC20 driver across all target platforms, with
the release consent and static OpenSSL features enabled and dev dependencies
excluded. The check rejects reqwest native TLS, native CA roots, or missing
Mozilla roots. Cargo features are additive: adding a dependency with reqwest's
default features can otherwise change the default TLS backend for ERC20 too.

## Test certificate rejection without network access

```sh
cargo test --locked -p ya-erc20-driver --test http_tls
```

The test starts a local TLS server with a generated self-signed certificate. It
uses the HTTP client constructed by `Web3RpcPool`, without injecting a custom
client, and requires a TLS `UnknownCA` alert. As a positive control, a client
explicitly trusting that certificate must successfully read the chain ID from
the same server. The test has a 15-second deadline.

## Test a real public certificate

```sh
cargo test --locked -p ya-erc20-driver --test http_tls \
  erc20_default_client_accepts_public_ca -- --ignored --nocapture
```

This read-only smoke test connects through the ERC20-created client to Publicnode
and checks Hoodi's chain ID, 560048. It has a 30-second deadline and is ignored in
normal CI because it depends on Internet access and an external RPC service.

On Linux, an additional check runs it with empty OpenSSL CA locations:

```sh
empty_ca_dir=$(mktemp -d)
SSL_CERT_FILE=/dev/null SSL_CERT_DIR="$empty_ca_dir" \
  cargo test --locked --offline -p ya-erc20-driver --test http_tls \
  erc20_default_client_accepts_public_ca -- --ignored --nocapture
rmdir "$empty_ca_dir"
```

`--offline` applies to Cargo dependency downloads; the test itself still makes
an HTTPS request. Together with the feature check, this verifies that the RPC
connection does not need the OpenSSL CA paths.
