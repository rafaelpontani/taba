# syntax=docker/dockerfile:1
#
# The Taba node image: the `taba` binary (static, linked against musl) on Alpine, with nothing else besides
# the CA certificates. The same binary does `taba init`, `taba cert generate` and `taba token
# generate` (doc/06-api.md, section 8.0.1).
#
#   docker build -t taba .
#   docker run --rm -v "$PWD/secrets:/out" taba init --out /out
#
# The `docker-compose.yml` next to it starts a complete cluster.

FROM rust:1-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# Static, linked against musl. With --target the RUSTFLAGS do not apply to procedural macros (which run
# at build time), which cannot be static.
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN triple="$(uname -m)-unknown-linux-musl" \
 && cargo build --release --locked -p taba-node --target "$triple" \
 && cp "target/$triple/release/taba" /taba

FROM alpine:3
RUN apk add --no-cache ca-certificates \
 && adduser -D -u 10001 taba \
 && mkdir -p /var/lib/taba /etc/taba/secrets \
 && chown taba:taba /var/lib/taba
COPY --from=build /taba /usr/local/bin/taba
# Unprivileged. The node data (storage.dir) and the secrets folder (secrets.dir) are volumes.
USER taba
VOLUME ["/var/lib/taba"]
EXPOSE 8080 8081 9090
ENTRYPOINT ["taba"]
