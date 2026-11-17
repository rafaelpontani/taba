# Build image of the fuzz targets for ClusterFuzzLite (.github/workflows/fuzzing.yml): the OSS-Fuzz Rust builder
# (nightly, cargo-fuzz, clang with the sanitizers), pinned by digest (D226). The targets are in
# crates/taba-node/fuzz (doc/en-US/27-ci-cd.md#fuzzing).
FROM gcr.io/oss-fuzz-base/base-builder-rust:v1@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
# The nightly of the image is older than the rustc the dependencies of the node require; a dated nightly keeps the
# build reproducible (move the date when a dependency asks for a newer rustc).
ENV RUSTUP_TOOLCHAIN=nightly-2026-11-01
RUN rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal --component rust-src
COPY . $SRC/taba
WORKDIR $SRC/taba
COPY .clusterfuzzlite/build.sh $SRC/
