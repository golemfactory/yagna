#!/bin/bash

set -e

# Create target directory
mkdir -p ../target/goth/bin

# Change to source directory
cd ../target/x86_64-unknown-linux-musl/debug

# Copy each binary if it exists
for binary in yagna exe-unit gftp golemsp ya-provider erc20_processor; do
    if [ -f "$binary" ]; then
        echo "Copying $binary..."
        cp "$binary" "../../goth/"
    else
        echo "Warning: $binary not found, skipping... Building goth docker can fail or outdated version of the binary can be used."
    fi
done

echo "Copying binaries completed." 