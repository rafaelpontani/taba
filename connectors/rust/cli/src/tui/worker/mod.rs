//! The workers (doc 34, section 4): `taba-client` is blocking, so the calls `update` asks for run on a few threads and
//! come back as a `Msg::Reply` marked with the request and the generation they were asked in.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use taba_client::{Call, Client, Error, Reply};

use crate::tui::api::ApiCall;
use crate::tui::msg::Msg;

/// What a worker calls. The real one is the `Client`; the tests give a scripted one.
pub trait Backend: Send + Sync {
    fn call(&self, call: &Call) -> Result<Reply, Error>;
}

impl Backend for Client {
    fn call(&self, call: &Call) -> Result<Reply, Error> {
        Client::call(self, call)
    }
}

/// A backend that can be replaced while the workers run: choosing another profile points every later call at
/// the new server.
pub struct SwitchBackend {
    current: std::sync::RwLock<Arc<dyn Backend>>,
}

impl SwitchBackend {
    pub fn new(first: Arc<dyn Backend>) -> SwitchBackend {
        SwitchBackend {
            current: std::sync::RwLock::new(first),
        }
    }

    pub fn switch(&self, next: Arc<dyn Backend>) {
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = next;
    }
}

impl Backend for SwitchBackend {
    fn call(&self, call: &Call) -> Result<Reply, Error> {
        let backend = Arc::clone(&self.current.read().unwrap_or_else(|e| e.into_inner()));
        backend.call(call)
    }
}

/// How many calls can be in flight at once.
pub const THREADS: usize = 4;

struct Job {
    id: u64,
    generation: u64,
    call: ApiCall,
}

pub struct Workers {
    jobs: Sender<Job>,
}

impl Workers {
    /// Starts the threads; they end when the `Workers` is dropped.
    pub fn start(backend: Arc<dyn Backend>, results: Sender<Msg>, threads: usize) -> Workers {
        let (jobs, queue) = std::sync::mpsc::channel::<Job>();
        let queue: Arc<Mutex<Receiver<Job>>> = Arc::new(Mutex::new(queue));
        for _ in 0..threads.max(1) {
            let queue = Arc::clone(&queue);
            let backend = Arc::clone(&backend);
            let results = results.clone();
            thread::spawn(move || loop {
                // The lock is held only to take the next job, never while the call runs.
                let job = match queue.lock() {
                    Ok(guard) => guard.recv(),
                    Err(_) => return,
                };
                let Ok(job) = job else { return };
                let result = backend.call(&job.call.to_call());
                let reply = Msg::Reply {
                    id: job.id,
                    generation: job.generation,
                    call: job.call,
                    result,
                };
                if results.send(reply).is_err() {
                    return;
                }
            });
        }
        Workers { jobs }
    }

    pub fn submit(&self, id: u64, generation: u64, call: ApiCall) {
        let _ = self.jobs.send(Job {
            id,
            generation,
            call,
        });
    }
}

#[cfg(test)]
mod tests;
