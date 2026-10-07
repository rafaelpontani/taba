//! O nó do Taba. O binário `taba` (`main.rs`) é só a linha de comando e os sinais;
//! tudo o mais mora aqui para os testes de integração subirem nós de verdade.
//!
//! A direção das dependências é `core ← storage ← engine ← cluster ← http ← node`
//! (doc 18, seção 17.3): cada módulo conhece só os da esquerda. `config` fica ao
//! lado: lê o arquivo e o ambiente e só depende de `core`.

pub mod cli;
pub mod cluster;
pub mod config;
pub mod core;
pub mod engine;
pub mod http;
pub mod node;
pub mod storage;
pub mod telemetry;
