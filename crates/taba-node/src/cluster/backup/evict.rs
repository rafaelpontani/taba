//! Disco cheio: a evicção (doc 23, "Backup em S3", seção 1.8).
//!
//! Acima de `backup.disk.evict-above` (85%) o nó apaga o **arquivo do corpo** das mensagens que já
//! têm referência no bucket, até o uso cair a `backup.disk.evict-to` (70%). A linha em `_topic.db`
//! fica, então a ordem "arquivo primeiro, linha depois" do armazenamento continua valendo. Só se
//! libera o que está em disco: mensagem só em memória não libera nada. Na leitura, o arquivo
//! ausente vira um `Range` no segmento (`fetch_body`).
//!
//! Durante o flush e o drain, ao chegar a `backup.disk.pause-at` (90%) a passagem da memória para o
//! disco **pausa** até a evicção liberar espaço, no máximo por `backup.drain.timeout`.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{Backup, lock};

/// A cada quantas evicções o uso do disco é conferido de novo.
const CHECK_EVERY: u32 = 32;

impl Backup {
    /// O uso do disco (%). A leitura real vale por 200 ms: o drain a pede a cada mensagem.
    fn usage(&self) -> u32 {
        if let Some(probe) = lock(&self.disk_probe).clone() {
            return probe();
        }
        let mut cached = lock(&self.disk_usage);
        if let Some((at, percent)) = *cached
            && at.elapsed() < Duration::from_millis(200)
        {
            return percent;
        }
        let percent = self.disk.disk_usage_percent().unwrap_or(0);
        *cached = Some((Instant::now(), percent));
        percent
    }

    /// Uma passada de evicção: só age acima de `evict-above`. Devolve quantos corpos apagou.
    pub(crate) fn evict_pass(&self) -> u64 {
        if self.usage() <= self.rules.evict_above {
            return 0;
        }
        self.evict_down_to(self.rules.evict_to)
    }

    /// Apaga corpos de mensagens salvas até o uso do disco chegar a `target` (%).
    pub(crate) fn evict_down_to(&self, target: u32) -> u64 {
        let mut evicted = 0u64;
        let mut since_check = 0;
        for topic in self.messages.topic_names() {
            let Ok(saved) = self.db.saved_of_topic(&topic) else {
                continue;
            };
            for (id, _) in saved {
                match self.disk.evict_body(&topic, &id) {
                    Ok(freed) if freed > 0 => {
                        evicted += 1;
                        self.counters.bodies_evicted.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::warn!(error = %e, topic = %topic, "backup: evicção falhou");
                        continue;
                    }
                }
                since_check += 1;
                if since_check >= CHECK_EVERY {
                    since_check = 0;
                    if self.usage() <= target {
                        return evicted;
                    }
                }
            }
        }
        if evicted > 0 {
            tracing::info!(
                evicted,
                usage = self.usage(),
                "backup: corpos de mensagens já salvas no S3 saíram do disco"
            );
        }
        evicted
    }

    /// Antes de passar uma mensagem da memória para o disco (drain e flush): com o disco em
    /// `pause-at`, espera a evicção liberar espaço, até `backup.drain.timeout`. Devolve `false` se
    /// desistiu (o nó segue o comportamento de sempre: falha por disco cheio).
    pub(crate) fn wait_for_disk(&self) -> bool {
        if self.usage() < self.rules.pause_at {
            return true;
        }
        let deadline = Instant::now() + self.rules.drain_timeout;
        let mut announced = false;
        loop {
            self.evict_down_to(self.rules.evict_to);
            if self.usage() < self.rules.pause_at {
                return true;
            }
            if !announced {
                tracing::warn!(
                    usage = self.usage(),
                    "backup: disco em {}% ou mais; a passagem da memória para o disco pausa até a evicção liberar espaço",
                    self.rules.pause_at
                );
                announced = true;
            }
            if Instant::now() >= deadline || self.cancel.is_cancelled() {
                tracing::error!("backup: o disco não liberou espaço no prazo; seguindo");
                return false;
            }
            // O envio ao S3 é o que cria mais corpos evictáveis.
            self.wake.notify_one();
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
