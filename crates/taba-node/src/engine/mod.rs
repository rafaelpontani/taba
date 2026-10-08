//! Tópicos, mensagens, listas de prontos e de invisíveis, entrega justa, FIFO e dead-letter
//! (doc 13, 14, 15 e 17). Cada tópico terá um escritor único: uma task com fila de
//! comandos (doc 18, seção 17.4). Por enquanto: o registro de tópicos (`topics`) e a
//! publicação e o consumo em memória (`messages`).

pub mod messages;
pub mod tokens;
pub mod topics;
