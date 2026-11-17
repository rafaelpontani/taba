#!/bin/bash -eu
# Builds the fuzz targets of crates/taba-node/fuzz inside the ClusterFuzzLite image (Dockerfile next to this file)
# and copies them to $OUT, where the runner picks them up. cargo-fuzz takes the sanitizer flags from the image.
cd "$SRC/taba/crates/taba-node"
cargo fuzz build -O --debug-assertions
targets="target/x86_64-unknown-linux-gnu/release"
[ -d "fuzz/$targets" ] && targets="fuzz/$targets"
for target in $(cargo fuzz list); do
  cp "$targets/$target" "$OUT/"
done
