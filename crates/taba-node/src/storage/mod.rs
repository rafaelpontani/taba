//! Memória, overflow em disco, SQLite por tópico, `sync.db` e `_node.db` (doc 02, seção 3.2): o
//! banco do nó (`db`), as definições de tópico (`topics`) e as mensagens em disco (`messages`).
//!
//! Os traits ficam aqui, na camada mais baixa de que o `engine` precisa.

pub mod db;
pub mod messages;
pub mod topics;

use std::io;
use std::path::Path;

/// Apaga o que há **dentro** de `dir`, mantendo a pasta. É o `development.resume-on-start=false`
/// (doc 10): o nó começa limpo.
pub fn wipe_dir_contents(dir: &Path) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Cria `storage.dir` se não existir e confere que dá para gravar nela. Se não der,
/// o nó não sobe (doc 10, `storage.dir`).
pub fn prepare_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".taba-write-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)
}

#[cfg(test)]
mod tests;
