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

FROM rust:1-alpine@sha256:557a94c82b485a14b569120e00867267108c734efe33f10cc31ca0cd5dd1eca9 AS build
RUN apk add --no-cache build-base
WORKDIR /src/rust
# Binário pequeno e estático. Com --target o RUSTFLAGS não vale para as macros de procedimento.
ENV RUSTFLAGS="-C target-feature=+crt-static" \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
    CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_PANIC=abort \
    CARGO_PROFILE_RELEASE_STRIP=symbols

# Camada 1, as dependências: entram só os manifestos e um esqueleto dos crates, então esta camada (a lenta) só é
# refeita quando um Cargo.toml ou o Cargo.lock mudam, não a cada edição do código.
COPY connectors/rust/Cargo.toml connectors/rust/Cargo.lock ./
COPY connectors/rust/client/Cargo.toml client/Cargo.toml
COPY connectors/rust/cli/Cargo.toml cli/Cargo.toml
RUN mkdir -p client/src cli/src \
 && echo "" > client/src/lib.rs \
 && echo "" > cli/src/lib.rs \
 && echo "fn main() {}" > cli/src/main.rs \
 && triple="$(uname -m)-unknown-linux-musl" \
 && cargo build --release --locked -p taba-cli --target "$triple"

# Camada 2, o código: os arquivos copiados são mais antigos que o esqueleto, então o touch faz o cargo recompilar
# os crates (só eles; as dependências ficam como estão).
COPY connectors/rust /src/rust
RUN triple="$(uname -m)-unknown-linux-musl" \
 && touch client/src/lib.rs cli/src/lib.rs cli/src/main.rs \
 && cargo build --release --locked -p taba-cli --target "$triple" \
 && cp "target/$triple/release/taba-cli" /taba-cli

FROM alpine:3@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6
RUN apk add --no-cache ca-certificates \
 && adduser -D -u 10001 taba
COPY --from=build /taba-cli /usr/local/bin/taba-cli
# Sem privilégios: o programa só fala com a API.
USER taba
ENTRYPOINT ["taba-cli"]
