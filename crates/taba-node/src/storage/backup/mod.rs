//! O que o backup opcional em S3 precisa do armazenamento (doc 23, "Backup em S3"): o depósito de
//! objetos (`store`), os nomes no bucket (`keys`), o formato do segmento (`segment`) e do diário
//! (`journal`), e o `_backup.db` local (`db`).
//!
//! Nada aqui fala com o cluster nem com o motor de mensagens: a coordenação mora em
//! `cluster::backup`. O S3 é só backup de falha grave; SQLite e memória seguem como o armazenamento
//! primário, e nenhuma publicação espera o S3.

pub mod db;
pub mod journal;
pub mod keys;
pub mod segment;
pub mod store;

pub use db::{BackupDb, Committed, Dirty, SavedRef, SegmentRow};
pub use store::{
    MemoryStore, Object, ObjectInfo, ObjectStore, Prefixed, S3Settings, S3Store, StoreError,
    StoreResult,
};
