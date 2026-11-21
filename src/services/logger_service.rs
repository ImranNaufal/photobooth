use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

static LOG_BUFFER: OnceLock<Arc<RwLock<VecDeque<String>>>> = OnceLock::new();
static FILE_LOCK: OnceLock<Arc<Mutex<File>>> = OnceLock::new();

#[derive(Clone)]
pub struct LoggerService;

#[derive(Clone)]
pub struct MakeMemoryAndFileWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeMemoryAndFileWriter {
    type Writer = MemoryAndFileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        let file = FILE_LOCK
            .get()
            .expect("LoggerService must be initialized before tracing")
            .clone();
        let buffer = LOG_BUFFER
            .get()
            .expect("LoggerService must be initialized before tracing")
            .clone();

        MemoryAndFileWriter { file, buffer }
    }
}

pub struct MemoryAndFileWriter {
    file: Arc<Mutex<File>>,
    buffer: Arc<RwLock<VecDeque<String>>>,
}

impl Write for MemoryAndFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // 1. Write to local file
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(buf);
            let _ = f.flush();
        }

        // 2. Also write to stdout if available
        let _ = std::io::stdout().write(buf);

        // 3. Store in in-memory ring buffer for remote web viewer
        if let Ok(s) = std::str::from_utf8(buf) {
            if let Ok(mut rb) = self.buffer.write() {
                if rb.len() >= 300 {
                    rb.pop_front();
                }
                rb.push_back(s.to_string());
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
        Ok(())
    }
}

impl LoggerService {
    /// Initialize the log directory, file, and in-memory ring buffer
    pub fn init() -> MakeMemoryAndFileWriter {
        let log_dir = Path::new("logs");
        if !log_dir.exists() {
            let _ = fs::create_dir_all(log_dir);
        }

        let log_path = log_dir.join("photobooth.log");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&log_path)
            .unwrap_or_else(|_| File::create(&log_path).expect("Failed to create log file"));

        let file_arc = Arc::new(Mutex::new(file));
        let buffer_arc = Arc::new(RwLock::new(VecDeque::with_capacity(350)));

        let _ = FILE_LOCK.set(file_arc);
        let _ = LOG_BUFFER.set(buffer_arc);

        MakeMemoryAndFileWriter
    }

    /// Retrieve the most recent log entries (up to 300 lines)
    pub fn get_recent_logs() -> Vec<String> {
        if let Some(buf) = LOG_BUFFER.get() {
            if let Ok(guard) = buf.read() {
                return guard.iter().cloned().collect();
            }
        }
        Vec::new()
    }

    /// Read the full log file content from disk
    pub fn read_full_log_file() -> std::io::Result<String> {
        fs::read_to_string("logs/photobooth.log")
    }

    /// Non-blocking alert to Discord / Telegram webhook
    #[allow(dead_code)]
    pub fn send_webhook_alert(webhook_url: &str, alert_text: &str) {
        if webhook_url.trim().is_empty() {
            return;
        }

        let url = webhook_url.to_string();
        let payload = serde_json::json!({
            "content": format!("🚨 **PHOTOBOOTH ALERT** 🚨\n```\n{}\n```", alert_text)
        });

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build();

            if let Ok(c) = client {
                let _ = c.post(&url).json(&payload).send().await;
            }
        });
    }
}
