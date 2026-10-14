//! Signals from the daemon to UIs (spec 02 §2.1).

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Signal {
    pub signal: String,
    pub data: Value,
}

pub struct Bus {
    tx: broadcast::Sender<Signal>,
}

impl Default for Bus {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(256);
        Bus { tx }
    }
}

impl Bus {
    pub fn emit(&self, signal: &str, data: Value) {
        log::debug!("signal {signal} {data}");
        let _ = self.tx.send(Signal { signal: signal.to_string(), data });
    }

    pub fn device(&self, signal: &str, id: &str) {
        self.emit(signal, json!({ "id": id }));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }
}
