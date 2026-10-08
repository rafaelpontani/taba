//! Membros, votação, freezing, canal, registro + commit, sincronização e drain
//! (doc 04, 05 e 13). Por enquanto o estado do nó e o drain local; o resto entra com o
//! protocolo de cluster (passos 4 a 9 de `08c-proximos-passos.md`).

pub mod drain;
pub mod state;
