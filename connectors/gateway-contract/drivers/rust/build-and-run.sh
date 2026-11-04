#!/bin/sh
# Inside the container: builds the driver (the target directory is a volume, so later runs are incremental) and runs it.
mkdir -p /app && cd /app && cp -r /repo/connectors/gateway-contract/drivers/rust/src /repo/connectors/gateway-contract/drivers/rust/Cargo.toml .
export CARGO_TARGET_DIR=/cache/target
cargo build --release 2>&1 | grep -E '^error|-->|^\s+\|' | head -40
version=$(grep -A1 'name = "aws-sdk-sqs"' Cargo.lock | tail -1 | cut -d'"' -f2)
exec /cache/target/release/gwdriver --sdk "aws-sdk-sqs-$version" "$@"
