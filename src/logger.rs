use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
};

pub static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

pub struct LoggerGuard;

impl Drop for LoggerGuard {
    fn drop(&mut self) {
        // Ensure log file is flushed and closed properly
        if let Ok(mut log_guard) = LOG_FILE.lock() {
            if let Some(ref mut file) = *log_guard {
                let _ = file.flush();
            }
        }
    }
}

pub fn init_logger() -> Result<LoggerGuard, std::io::Error> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("agnt-log.txt")?;

    if let Ok(mut log_guard) = LOG_FILE.lock() {
        *log_guard = Some(file);
        // Log initialization message directly
        if let Some(ref mut file) = *log_guard {
            let _ = writeln!(
                file,
                "[{}] === AGNT Logger Initialized ===",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
            );
            let _ = file.flush();
        }
    }

    Ok(LoggerGuard)
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let msg = format!($($arg)*);

        // Write to log file if available
        if let Ok(mut log_guard) = $crate::logger::LOG_FILE.lock() {
            if let Some(ref mut file) = *log_guard {
                let _ = writeln!(file, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), msg);
                let _ = file.flush();
            }
        }
    }};
}
