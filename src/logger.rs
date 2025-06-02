use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
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
    // Get home directory and create ~/.agnt/logs.txt path
    let log_path = if let Some(home_dir) = dirs::home_dir() {
        let agnt_dir = home_dir.join(".agnt");
        // Create directory if it doesn't exist
        fs::create_dir_all(&agnt_dir)?;
        agnt_dir.join("logs.txt")
    } else {
        // Fallback to current directory if home directory cannot be determined
        PathBuf::from("agnt-log.txt")
    };

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;

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
