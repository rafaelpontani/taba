//! O ponto de ligação do motor com o backup opcional em S3 (doc 23, "Backup em S3").
//!
//! O motor só **avisa** o que aconteceu com cada mensagem, de forma barata e sem nunca esperar o S3:
//! quem implementa ([`BackupSink`], em `cluster::backup`) grava uma linha no `_backup.db` local. Sem
//! backup configurado, nenhum aviso é dado.

/// O que o motor avisa ao backup. Chamado com a trava do tópico: tem de ser rápido e nunca falhar
/// para quem chama (um erro do backup vira log, nunca erro de publicação).
pub trait BackupSink: Send + Sync + 'static {
    /// Esta cópia da mensagem foi guardada no nó (publicação, restauração, recuperação). `owned`: o
    /// nó é o dono (primário do bloco), o único que envia ao S3.
    fn stored(&self, topic: &str, id: &str, owned: bool);

    /// A mensagem morreu no nó: ack, `ttl` ou saída para a dead-letter. `owned`: era dono.
    fn removed(&self, topic: &str, id: &str, owned: bool);

    /// O estado mutável da mensagem mudou (entrega, invisibilidade).
    fn changed(&self, topic: &str, id: &str, owned: bool);

    /// O nó passou a ser o dono da mensagem (mudança de membros): se ela ainda não está no S3,
    /// entra na fila de envio.
    fn adopted(&self, topic: &str, id: &str);

    /// O nó deixou de ser o dono: a fila de envio dele esquece a mensagem (o novo dono a envia).
    fn released(&self, topic: &str, id: &str);

    /// O corpo da mensagem não está no disco (a evicção o apagou): busca no S3 por `Range`. `None`
    /// sem backup, sem referência ou com o S3 fora do ar.
    fn fetch_body(&self, topic: &str, id: &str) -> Option<Vec<u8>>;

    /// Chamado antes de cada passagem da memória para o disco no drain e no flush: se o disco
    /// chegou ao limite de pausa, espera a evicção liberar espaço (até o limite do backup). Devolve
    /// `false` quando desistiu de esperar.
    fn before_disk_write(&self) -> bool;
}
