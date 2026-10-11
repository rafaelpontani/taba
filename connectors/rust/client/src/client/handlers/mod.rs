//! O consumo com *handler* (D140, doc 19 §18.3): `consume_with` (e, com a *feature* `serde`,
//! `consume_typed`) lê da fila com long polling, entrega cada mensagem a um [`Delivery`], apaga a
//! mensagem se o *handler* devolve `Ok` e `delete` continua `true`, e **renova sozinho a
//! invisibilidade** (uma segunda *thread*, a cada um terço dela) enquanto o *handler* roda. Com a
//! *feature* `serde`, `publish_object` serializa o objeto antes de publicar.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::{Client, ConsumeOptions, Error, Message, VisibilityResponse};

/// Uma mensagem por vez (o `maxMessages` do consumo com *handler*).
pub const DEFAULT_MAX_MESSAGES: u32 = 1;
/// O long polling do consumo com *handler*, em segundos.
pub const DEFAULT_WAIT_TIME: u32 = 20;
/// A invisibilidade do consumo com *handler*, em segundos, que a renovação mantém.
pub const DEFAULT_VISIBILITY_TIMEOUT: u32 = 120;

/// O erro que o *handler* devolve; a mensagem dele **não** é apagada.
pub type HandlerError = Box<dyn std::error::Error>;

/// O que o consumo com *handler* devolve de errado: a API (ou o próprio cliente) ou o *handler*.
#[derive(Debug)]
pub enum ConsumeError {
    Api(Error),
    Handler(HandlerError),
}

impl fmt::Display for ConsumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConsumeError::Api(e) => write!(f, "{e}"),
            ConsumeError::Handler(e) => write!(f, "handler: {e}"),
        }
    }
}

impl std::error::Error for ConsumeError {}

impl From<Error> for ConsumeError {
    fn from(e: Error) -> ConsumeError {
        ConsumeError::Api(e)
    }
}

/// O que o *handler* recebe: a mensagem inteira, o corpo (já deserializado no `consume_typed`; `()`
/// no `consume_with`), `delete` (padrão `true`) e [`Delivery::change_visibility`].
pub struct Delivery<'a, T = ()> {
    pub message: &'a Message,
    pub body: T,
    /// `true` (o padrão): a mensagem é apagada (`ack`) se o *handler* devolve `Ok`. Com `false`, ela
    /// fica na fila e volta a ficar visível ao fim da invisibilidade.
    pub delete: bool,
    client: &'a Client,
    topic: &'a str,
    lease: &'a AtomicU32,
    lost: &'a AtomicBool,
}

impl<T> Delivery<'_, T> {
    /// Muda a invisibilidade para `seconds` a partir de agora; o valor passa a ser o da renovação.
    pub fn change_visibility(&self, seconds: u32) -> Result<VisibilityResponse, Error> {
        let reply = self.client.change_visibility(
            self.topic,
            &self.message.id,
            &self.message.receipt_token,
            seconds,
        )?;
        self.lease.store(seconds, Ordering::SeqCst);
        Ok(reply)
    }

    /// Os segundos que a renovação automática pede a cada volta.
    pub fn lease_seconds(&self) -> u32 {
        self.lease.load(Ordering::SeqCst)
    }

    /// `true` se o servidor recusou uma renovação (a mensagem já voltou à fila ou foi apagada).
    pub fn lease_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }
}

/// Os padrões do consumo com *handler*; o que a pessoa informou vale.
fn with_defaults(options: &ConsumeOptions) -> ConsumeOptions {
    let mut options = options.clone();
    options.max_messages.get_or_insert(DEFAULT_MAX_MESSAGES);
    options.wait_time.get_or_insert(DEFAULT_WAIT_TIME);
    options
        .visibility_timeout
        .get_or_insert(DEFAULT_VISIBILITY_TIMEOUT);
    options
}

/// O intervalo da renovação: um terço da invisibilidade, para sobrar tempo de repetir se uma
/// renovação falhar antes de a mensagem ser liberada a outro consumidor.
fn renewal_interval(lease_seconds: u32) -> Duration {
    Duration::from_secs_f64(f64::from(lease_seconds.max(1)) / 3.0)
}

impl Client {
    /// Lê da fila e chama o `handler` para cada mensagem (veja o módulo). Devolve quantas foram
    /// tratadas. Se o `handler` devolve `Err`, a mensagem não é apagada e o erro sobe.
    pub fn consume_with<H>(
        &self,
        topic: &str,
        options: &ConsumeOptions,
        mut handler: H,
    ) -> Result<usize, ConsumeError>
    where
        H: FnMut(&mut Delivery<'_, ()>) -> Result<(), HandlerError>,
    {
        let options = with_defaults(options);
        let lease = options
            .visibility_timeout
            .unwrap_or(DEFAULT_VISIBILITY_TIMEOUT);
        let messages = self.consume(topic, &options)?;
        let mut handled = 0;
        for message in &messages {
            self.handle_one(topic, lease, message, (), &mut handler)?;
            handled += 1;
        }
        Ok(handled)
    }

    fn handle_one<T, H>(
        &self,
        topic: &str,
        lease_seconds: u32,
        message: &Message,
        body: T,
        handler: &mut H,
    ) -> Result<(), ConsumeError>
    where
        H: FnMut(&mut Delivery<'_, T>) -> Result<(), HandlerError>,
    {
        let lease = AtomicU32::new(lease_seconds);
        let lost = AtomicBool::new(false);
        let stop = (Mutex::new(false), Condvar::new());
        let mut delivery = Delivery {
            message,
            body,
            delete: true,
            client: self,
            topic,
            lease: &lease,
            lost: &lost,
        };
        let result = std::thread::scope(|scope| {
            scope.spawn(|| self.keep_alive(topic, message, &lease, &lost, &stop));
            let result = handler(&mut delivery);
            *stop.0.lock().unwrap() = true;
            stop.1.notify_all();
            result
        });
        result.map_err(ConsumeError::Handler)?;
        if delivery.delete {
            match self.ack(topic, &message.id, &message.receipt_token) {
                // A mensagem já voltou à fila (prazo vencido, `409`) ou já foi embora (`404`).
                Err(e) if e.status == 409 || e.status == 404 => {}
                other => other?,
            }
        }
        Ok(())
    }

    /// A segunda *thread*: renova a invisibilidade até o *handler* terminar. Nunca falha.
    fn keep_alive(
        &self,
        topic: &str,
        message: &Message,
        lease: &AtomicU32,
        lost: &AtomicBool,
        stop: &(Mutex<bool>, Condvar),
    ) {
        loop {
            let interval = renewal_interval(lease.load(Ordering::SeqCst));
            let guard = stop.0.lock().unwrap();
            let (guard, _) = stop
                .1
                .wait_timeout_while(guard, interval, |stopped| !*stopped)
                .unwrap();
            if *guard {
                return;
            }
            drop(guard);
            if let Err(e) = self.change_visibility(
                topic,
                &message.id,
                &message.receipt_token,
                lease.load(Ordering::SeqCst),
            ) {
                if e.status == 409 || e.status == 404 {
                    // Já voltou à fila ou foi apagada: renovar não adianta mais.
                    lost.store(true, Ordering::SeqCst);
                    return;
                }
                // Falha passageira (rede, 5xx): tenta de novo na próxima volta.
            }
        }
    }
}

#[cfg(feature = "serde")]
impl Client {
    /// Serializa `value` em JSON e publica como `application/json`. Para serializar por conta
    /// própria, use o `publish` com os bytes.
    pub fn publish_object<T: serde::Serialize>(
        &self,
        topic: &str,
        value: &T,
    ) -> Result<super::PublishResponse, Error> {
        let json = serde_json::to_vec(value)
            .map_err(|e| Error::client("client.invalid-request", format!("objeto: {e}")))?;
        self.publish(topic, &json, &super::PublishOptions::default())
    }

    /// O [`Client::consume_with`] com o corpo já deserializado como `T` em `delivery.body`. Se o
    /// corpo não é um `T`, devolve `client.invalid-response` antes de chamar o *handler*, e a
    /// mensagem não é apagada.
    pub fn consume_typed<T, H>(
        &self,
        topic: &str,
        options: &ConsumeOptions,
        mut handler: H,
    ) -> Result<usize, ConsumeError>
    where
        T: serde::de::DeserializeOwned,
        H: FnMut(&mut Delivery<'_, T>) -> Result<(), HandlerError>,
    {
        let options = with_defaults(options);
        let lease = options
            .visibility_timeout
            .unwrap_or(DEFAULT_VISIBILITY_TIMEOUT);
        let messages = self.consume(topic, &options)?;
        let mut handled = 0;
        for message in &messages {
            let body: T = serde_json::from_slice(&message.body).map_err(|e| {
                Error::client(
                    "client.invalid-response",
                    format!("corpo da mensagem {}: {e}", message.id),
                )
            })?;
            self.handle_one(topic, lease, message, body, &mut handler)?;
            handled += 1;
        }
        Ok(handled)
    }
}

#[cfg(test)]
mod tests;
