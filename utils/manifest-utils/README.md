# ya-manifest-utils

Computation Payload Manifest utilities mainly related to [GAP-4](https://github.com/golemfactory/golem-architecture/blob/master/gaps/gap-4_comp_manifest/gap-4_comp_manifest.md)
and [GAP-5](https://github.com/golemfactory/golem-architecture/blob/master/gaps/gap-5_payload_manifest/gap-5_payload_manifest.md).

## Computation Payload Manifest schema

Computation Payload Manifest schema can be generated using dedicated binary:
```sh
# cd utils/manifest-utils
cargo run -p ya-manifest-utils --bin schema --features schema > manifest.schema.json
```