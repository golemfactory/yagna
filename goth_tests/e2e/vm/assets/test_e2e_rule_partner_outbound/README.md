# Signing node descriptor

Node dedscriptor might require update in case of certificate expiration or change of node id.
Updated node descriptor needs to be signed. To do so perform following steps:

Checkout golem-certificate and install cmdline tool:

```sh
git clone git@github.com:golemfactory/golem-certificate.git
cd golem-certificate/cli
cargo install --path .
cd -
```

Extract `partner-keypair.key` from `ya-manifest-test-utils` project:

```sh
tar -xf ../../../../../utils/manifest-utils/test-utils/resources/test/certificates.tar partner-keypair.key partner-certificate.signed.json
```

Sign `node-descriptor.json`

```sh
golem-certificate-cli sign node-descriptor.json partner-certificate.signed.json partner-keypair.key
```

Commit generated `node-descriptor.signed.json` file.
