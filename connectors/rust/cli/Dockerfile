# syntax=docker/dockerfile:1
#
# A imagem do taba-cli (doc/19b-cli.md, seção 18.11.7): mínima, em Alpine, só com o binário
# estático (ligado à musl) e os certificados de CA do sistema.
#
# O contexto é a raiz do repositório (o Dockerfile copia só a pasta connectors/rust/, o workspace do Cargo
# dos conectores):
#   docker buildx build -f connectors/rust/cli/Dockerfile -t taba-cli .
#   docker buildx build --platform linux/amd64,linux/arm64 -f connectors/rust/cli/Dockerfile -t taba-cli .
#
# O uso é o do programa: o que vem depois do nome da imagem são os comandos.
#   docker run --rm -e TABA_CLI_URL=http://taba:8080 taba-cli topic list

FROM rust:1-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /src/rust
COPY connectors/rust /src/rust
# Binário pequeno e estático. Com --target o RUSTFLAGS não vale para as macros de procedimento.
ENV RUSTFLAGS="-C target-feature=+crt-static" \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
    CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_PANIC=abort \
    CARGO_PROFILE_RELEASE_STRIP=symbols
RUN triple="$(uname -m)-unknown-linux-musl" \
 && cargo build --release --locked -p taba-cli --target "$triple" \
 && cp "target/$triple/release/taba-cli" /taba-cli

FROM alpine:3
RUN apk add --no-cache ca-certificates \
 && adduser -D -u 10001 taba
COPY --from=build /taba-cli /usr/local/bin/taba-cli
# Sem privilégios: o programa só fala com a API.
USER taba
ENTRYPOINT ["taba-cli"]
