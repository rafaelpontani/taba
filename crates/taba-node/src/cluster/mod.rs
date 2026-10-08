//! Membros, anel, protocolo entre nós, canal, votação, freezing, sincronização e drain (doc 04, 05
//! e 13). Por enquanto: o estado do nó e o drain local, a tabela de membros com o ingresso de um
//! nó novo em duas fases, o protocolo (os tipos das chamadas entre nós) e o `Transport`, que
//! tem só a versão em memória, a dos testes (doc 18, seção 17.5).

pub mod drain;
pub mod members;
pub mod protocol;
pub mod service;
pub mod state;
pub mod transport;
