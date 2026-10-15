//! Consuming with a *handler* (D140, doc 19 §18.3): `consume_with` (and, with the `serde` *feature*,
//! `consume_typed`) reads from the queue with long polling, hands each message to a [`Delivery`], deletes the
//! message if the *handler* returns `Ok` and `delete` is still `true`, and **renews the
//! invisibility by itself** (a second *thread*, every third of it) while the *handler* runs. With the
//! `serde` *feature*, `publish_object` serializes the object before publishing.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::{Client, ConsumeOptions, Error, Message, VisibilityResponse};

/// One message at a time (the `maxMessages` of consuming with a *handler*).
pub const DEFAULT_MAX_MESSAGES: u32 = 1;
/// The long polling of consuming with a *handler*, in seconds.
pub const DEFAULT_WAIT_TIME: u32 = 20;
/// The invisibility of consuming with a *handler*, in seconds, which the renewal keeps.
pub const DEFAULT_VISIBILITY_TIMEOUT: u32 = 120;

/// The error the *handler* returns; its message is **not** deleted.
pub type HandlerError = Box<dyn std::error::Error>;

/// What consuming with a *handler* returns when it goes wrong: the API (or the client itself) or the *handler*.
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

/// What the *handler* receives: the whole message, the body (already deserialized in `consume_typed`; `()`
/// in `consume_with`), `delete` (default `true`) and [`Delivery::change_visibility`].
pub struct Delivery<'a, T = ()> {
    pub message: &'a Message,
    pub body: T,
    /// `true` (the default): the message is deleted (`ack`) if the *handler* returns `Ok`. With `false`, it
    /// stays in the queue and becomes visible again at the end of the invisibility.
    pub delete: bool,
    client: &'a Client,
    topic: &'a str,
    lease: &'a AtomicU32,
    lost: &'a AtomicBool,
}

impl<T> Delivery<'_, T> {
    /// Changes the invisibility to `seconds` from now on; the value becomes the renewal's.
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

    /// The seconds the automatic renewal requests on each round.
    pub fn lease_seconds(&self) -> u32 {
        self.lease.load(Ordering::SeqCst)
    }

    /// `true` if the server refused a renewal (the message already went back to the queue or was deleted).
    pub fn lease_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }
}

/// The defaults of consuming with a *handler*; what the caller set applies.
fn with_defaults(options: &ConsumeOptions) -> ConsumeOptions {
    let mut options = options.clone();
    options.max_messages.get_or_insert(DEFAULT_MAX_MESSAGES);
    options.wait_time.get_or_insert(DEFAULT_WAIT_TIME);
    options
        .visibility_timeout
        .get_or_insert(DEFAULT_VISIBILITY_TIMEOUT);
    options
}

/// The renewal interval: a third of the invisibility, so there is time left to retry if a
/// renewal fails before the message is released to another consumer.
fn renewal_interval(lease_seconds: u32) -> Duration {
    Duration::from_secs_f64(f64::from(lease_seconds.max(1)) / 3.0)
}

impl Client {
    /// Reads from the queue and calls `handler` for each message (see the module). Returns how many were
    /// handled. If `handler` returns `Err`, the message is not deleted and the error propagates.
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
                // The message already went back to the queue (deadline expired, `409`) or is already gone (`404`).
                Err(e) if e.status == 409 || e.status == 404 => {}
                other => other?,
            }
        }
        Ok(())
    }

    /// The second *thread*: renews the invisibility until the *handler* finishes. Never fails.
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
                    // It already went back to the queue or was deleted: renewing is no longer of any use.
                    lost.store(true, Ordering::SeqCst);
                    return;
                }
                // Transient failure (network, 5xx): tries again on the next round.
            }
        }
    }
}

#[cfg(feature = "serde")]
impl Client {
    /// Serializes `value` to JSON and publishes it as `application/json`. To serialize on your
    /// own, use `publish` with the bytes.
    pub fn publish_object<T: serde::Serialize>(
        &self,
        topic: &str,
        value: &T,
    ) -> Result<super::PublishResponse, Error> {
        let json = serde_json::to_vec(value)
            .map_err(|e| Error::client("client.invalid-request", format!("objeto: {e}")))?;
        self.publish(topic, &json, &super::PublishOptions::default())
    }

    /// [`Client::consume_with`] with the body already deserialized as `T` in `delivery.body`. If the
    /// body is not a `T`, it returns `client.invalid-response` before calling the *handler*, and the
    /// message is not deleted.
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
                    format!("message body {}: {e}", message.id),
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
