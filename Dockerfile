# syntax=docker/dockerfile:1
#
# A imagem do nó do Taba: o binário `taba` (estático, ligado à musl) em Alpine, sem mais nada além
# dos certificados de CA. O mesmo binário faz `taba init`, `taba cert generate` e `taba token
# generate` (doc/06-api.md, seção 8.0.1).
#
#   docker build -t taba .
#   docker run --rm -v "$PWD/secrets:/out" taba init --out /out
#
# O `docker-compose.yml` ao lado sobe um cluster completo de três nós.

FROM rust:1-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# Estático, ligado à musl. Com --target o RUSTFLAGS não vale para as macros de procedimento (que rodam
# no build), que não podem ser estáticas.
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
# Sem privilégios. Os dados do nó (storage.dir) e a pasta de segredos (secrets.dir) são volumes.
USER taba
VOLUME ["/var/lib/taba"]
EXPOSE 8080 8081 9090
ENTRYPOINT ["taba"]
