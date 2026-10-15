//! Topics, messages, ready and invisible lists, fair delivery, FIFO and dead-letter
//! (doc 13, 14, 15 and 17). Each topic will have a single writer: a task with a command
//! queue (doc 18, section 17.4). For now: the topic registry (`topics`) and the
//! in-memory publish and consume (`messages`).

pub mod messages;
pub mod tokens;
pub mod topics;
