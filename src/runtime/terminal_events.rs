use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeEvent {
    TerminalOutputChanged {
        terminal_id: String,
        seq: u64,
    },
    TerminalExited {
        terminal_id: String,
    },
    TerminalTitleChanged {
        terminal_id: String,
        title: String,
    },
    ShellCommandStarted {
        task_id: Option<usize>,
        command_id: String,
        command: String,
        cwd: PathBuf,
    },
    ShellCommandFinished {
        task_id: Option<usize>,
        command_id: String,
        output_tail: String,
    },
    ShellCommandFailed {
        task_id: Option<usize>,
        command_id: String,
        error: String,
    },
    CodingOutputChanged {
        session_id: String,
        task_id: usize,
        seq: u64,
    },
}

pub(crate) struct TerminalEventBus {
    tx: broadcast::Sender<RuntimeEvent>,
}

impl TerminalEventBus {
    pub(crate) fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub(crate) fn publish(&self, event: RuntimeEvent) {
        let _ = self.tx.send(event);
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.tx.subscribe()
    }
}

static GLOBAL_TERMINAL_EVENT_BUS: OnceLock<Arc<TerminalEventBus>> = OnceLock::new();
static RUNTIME_LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn global_terminal_event_bus() -> Arc<TerminalEventBus> {
    GLOBAL_TERMINAL_EVENT_BUS
        .get_or_init(|| Arc::new(TerminalEventBus::new(1024)))
        .clone()
}

pub(crate) fn log_runtime_event(stage: &str, message: impl AsRef<str>) {
    let Some(home_dir) = dirs::home_dir() else {
        return;
    };
    let log_dir = home_dir.join(".one").join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let log_path = log_dir.join("terminal-events.log");
    let Ok(_guard) = RUNTIME_LOG_LOCK.get_or_init(|| Mutex::new(())).lock() else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    else {
        return;
    };
    let message = message.as_ref().replace('\n', "\\n");
    let _ = std::io::Write::write_all(
        &mut file,
        format!(
            "{} [{}] {}\n",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
            stage,
            message
        )
        .as_bytes(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn terminal_event_bus_delivers_events_to_subscribers() {
        let bus = TerminalEventBus::new(8);
        let mut rx = bus.subscribe();
        bus.publish(RuntimeEvent::TerminalOutputChanged {
            terminal_id: "term-1".to_string(),
            seq: 7,
        });

        let received = rx.recv().await.unwrap();
        assert_eq!(
            received,
            RuntimeEvent::TerminalOutputChanged {
                terminal_id: "term-1".to_string(),
                seq: 7,
            }
        );
    }
}
