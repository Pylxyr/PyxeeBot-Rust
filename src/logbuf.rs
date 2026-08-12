use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

const MAX_LINES: usize = 40;

/// Recent WARN+ log lines for an owner-alert DM — reuses tracing_subscriber's formatter via MakeWriter.
#[derive(Clone)]
pub struct RecentLogs(Arc<Mutex<VecDeque<String>>>);

impl RecentLogs {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LINES))))
    }

    /// Oldest-first snapshot, one line per entry, ready to drop into a code block.
    pub fn snapshot(&self) -> String {
        self.0
            .lock()
            .expect("recent logs mutex poisoned")
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn push_line(&self, line: &str) {
        let mut buf = self.0.lock().expect("recent logs mutex poisoned");
        if buf.len() >= MAX_LINES {
            buf.pop_front();
        }
        buf.push_back(line.trim_end().to_owned());
    }
}

impl Default for RecentLogs {
    fn default() -> Self {
        Self::new()
    }
}

/// The actual io::Write sink handed out per-event by MakeWriter.
pub struct RecentLogsWriter(RecentLogs);

impl Write for RecentLogsWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(text) = std::str::from_utf8(buf) {
            for line in text.lines().filter(|l| !l.is_empty()) {
                self.0.push_line(line);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RecentLogs {
    type Writer = RecentLogsWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RecentLogsWriter(self.clone())
    }
}
