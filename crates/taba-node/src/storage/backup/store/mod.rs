//! O depósito de objetos do backup (doc 23, "Backup em S3"): o trait [`ObjectStore`], o depósito
//! em memória dos testes ([`MemoryStore`]), o cliente do S3 ([`S3Store`]) e o prefixo base
//! ([`Prefixed`]).
//!
//! O S3 é **opcional** e nunca entra no caminho de publicação: quem usa o depósito trata cada erro
//! como "o backup está fora do ar" e segue.

mod memory;
mod s3;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub use memory::MemoryStore;
pub use s3::{S3Settings, S3Store};

/// O erro de uma chamada ao depósito.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StoreError {
    /// O depósito não respondeu (rede, credenciais, serviço fora do ar).
    #[error("o depósito de objetos não respondeu: {0}")]
    Unavailable(String),
    /// O depósito não suporta a escrita condicional (`If-None-Match`, `If-Match`).
    #[error("o depósito não suporta escrita condicional: {0}")]
    Unsupported(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = StoreResult<T>> + Send + 'a>>;

/// O que se sabe de um objeto sem baixá-lo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    /// Muda a cada gravação com conteúdo diferente: o batimento dos nós depende disto.
    pub etag: String,
}

/// Um objeto baixado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub body: Vec<u8>,
    pub etag: String,
}

/// O que o backup pede ao S3 (ou ao MinIO). Todas as chaves são relativas ao prefixo base.
pub trait ObjectStore: Send + Sync + 'static {
    /// Grava (ou substitui) o objeto e devolve o `etag` novo.
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String>;

    /// Cria o objeto só se ele ainda não existe (`If-None-Match: *`). `false`: já existia.
    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool>;

    /// Substitui o objeto só se o `etag` atual é `etag` (`If-Match`). `false`: mudou ou sumiu.
    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool>;

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>>;

    /// `len` bytes a partir de `start` (`Range`). `None`: o objeto não existe. O fim passa do
    /// tamanho sem erro.
    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>>;

    /// Os últimos `len` bytes do objeto (para ler o rodapé de um segmento sem baixá-lo).
    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>>;

    /// Os objetos cujo nome começa com `prefix`, em ordem de nome.
    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>>;

    /// Apaga o objeto. Apagar o que não existe não é erro.
    fn delete(&self, key: &str) -> StoreFuture<'_, ()>;
}

/// Põe um prefixo base (`backup.s3.prefix`) na frente de todas as chaves.
pub struct Prefixed {
    inner: Arc<dyn ObjectStore>,
    prefix: String,
}

impl Prefixed {
    /// `prefix` sem barra no começo e no fim; vazio, não muda nada.
    pub fn new(inner: Arc<dyn ObjectStore>, prefix: &str) -> Self {
        let prefix = prefix.trim_matches('/');
        Self {
            inner,
            prefix: if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}/")
            },
        }
    }

    fn full(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }
}

impl ObjectStore for Prefixed {
    fn put(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, String> {
        self.inner.put(&self.full(key), body)
    }

    fn put_if_absent(&self, key: &str, body: Vec<u8>) -> StoreFuture<'_, bool> {
        self.inner.put_if_absent(&self.full(key), body)
    }

    fn put_if_match(&self, key: &str, body: Vec<u8>, etag: &str) -> StoreFuture<'_, bool> {
        self.inner.put_if_match(&self.full(key), body, etag)
    }

    fn get(&self, key: &str) -> StoreFuture<'_, Option<Object>> {
        self.inner.get(&self.full(key))
    }

    fn get_range(&self, key: &str, start: u64, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        self.inner.get_range(&self.full(key), start, len)
    }

    fn get_suffix(&self, key: &str, len: u64) -> StoreFuture<'_, Option<Vec<u8>>> {
        self.inner.get_suffix(&self.full(key), len)
    }

    fn list(&self, prefix: &str) -> StoreFuture<'_, Vec<ObjectInfo>> {
        let strip = self.prefix.len();
        let fut = self.inner.list(&self.full(prefix));
        Box::pin(async move {
            let mut items = fut.await?;
            for item in &mut items {
                item.key = item.key[strip..].to_owned();
            }
            Ok(items)
        })
    }

    fn delete(&self, key: &str) -> StoreFuture<'_, ()> {
        self.inner.delete(&self.full(key))
    }
}

#[cfg(test)]
mod tests;
