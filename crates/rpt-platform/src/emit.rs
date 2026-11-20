//! Event emission to the UI plus the port of `RemoteLogger.ts` (which now
//! also goes to `tracing`).

use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::types::PlatformEvent;

pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Emitter {
    tx: Sender<PlatformEvent>,
    wake: WakeFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Emitter {
    pub fn new(tx: Sender<PlatformEvent>, wake: WakeFn) -> Self {
        Self { tx, wake }
    }

    pub fn send(&self, ev: PlatformEvent) {
        if self.tx.send(ev).is_ok() {
            (self.wake)();
        }
    }

    /// Equivalent of upstream `logger.write("error [Module] message")`.
    pub fn log(&self, level: Level, module: &str, msg: &str) {
        let lvl = match level {
            Level::Debug => {
                tracing::debug!(target: "rpt_platform", "[{module}] {msg}");
                "debug"
            }
            Level::Info => {
                tracing::info!(target: "rpt_platform", "[{module}] {msg}");
                "info"
            }
            Level::Warn => {
                tracing::warn!(target: "rpt_platform", "[{module}] {msg}");
                "warn"
            }
            Level::Error => {
                tracing::error!(target: "rpt_platform", "[{module}] {msg}");
                "error"
            }
        };
        let time = chrono::Local::now().format("%H:%M:%S");
        self.send(PlatformEvent::LogEntry { message: format!("[{time}] {lvl} [{module}] {msg}\n") });
    }
}
