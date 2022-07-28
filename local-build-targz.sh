#!/bin/bash

# Example usage `./local-build-targz.sh ubuntu v0.11.0-rc14` from yagna directory
# Result build will be named the same as CI build with tag `pre-rel-local-v0.10-rc3`
# You need sudo to install musl and rust musl target.

if ! command -v musl-gcc --h &> /dev/null
then
    echo "musl-gcc could not be found. Install it with:"
    echo "sudo apt-get install musl musl-tools"
    exit
fi

rustup target add x86_64-unknown-linux-musl

export OPENSSL_STATIC=1

# Required by pack-build.sh script
export OS_NAME=$1
export GITHUB_REF=pre-rel-local-$2

cargo build --release --features static-openssl --target x86_64-unknown-linux-musl
(cd core/gftp && cargo build --bin gftp -p gftp --features bin --release --target x86_64-unknown-linux-musl)
(cd golem_cli && cargo build --bin golemsp -p golemsp --release --target x86_64-unknown-linux-musl)
(cd agent/provider && cargo build --bin ya-provider -p ya-provider --release --target x86_64-unknown-linux-musl)
(cd exe-unit && cargo build --bin exe-unit -p ya-exe-unit --release --features openssl/vendored --target x86_64-unknown-linux-musl)

bash .ci/pack-build.sh

CURRENT_DIR=`pwd`
SUBNET="hybrid"
YAGNA_VERSION=${GITHUB_REF}
RELEASE_DIR="${CURRENT_DIR}/releases"
PROVIDER_BINARY_PATH="${RELEASE_DIR}/golem-provider-linux-${YAGNA_VERSION}.tar.gz"
REQUESTOR_BINARY_PATH="${RELEASE_DIR}/golem-requestor-linux-${YAGNA_VERSION}.tar.gz"

echo ""
echo "Binaries generated in: ${RELEASE_DIR}"
echo ""
echo "To update devnet ${SUBNET} run following command from yagna-testnet-scripts/ansible:"

echo "ansible-playbook -i envs/production/${SUBNET} \
--extra-vars=\"{\
ya_provider_yagna_url: ${PROVIDER_BINARY_PATH}, \
ya_provider_yagna_version: ${YAGNA_VERSION}, \
checker_yagna_url: ${REQUESTOR_BINARY_PATH}, \
checker_yagna_version: ${YAGNA_VERSION}\
}\" \
play_ya_provider.yml"
