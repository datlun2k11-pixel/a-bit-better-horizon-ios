//! Boot logging macros and functions for iOS crash diagnosis.
//! This module is included in both horizon-client and horizon-client-lib crates.

/// Macro for detailed boot logging with automatic file/line capture.
#[macro_export]
macro_rules! boot_log {
    ($($arg:tt)*) => {
        #[cfg(target_os = "ios")]
        crate::boot_logging::boot_log_detailed(&format!($($arg)*), file!(), line!());
        #[cfg(not(target_os = "ios"))]
        crate::boot_logging::boot_log(&format!($($arg)*));
    };
}

/// Macro for boot logging with captured backtrace.
#[macro_export]
macro_rules! boot_log_bt {
    ($($arg:tt)*) => {
        #[cfg(target_os = "ios")]
        {
            let bt = std::backtrace::Backtrace::capture();
            crate::boot_logging::boot_log_with_backtrace(&format!($($arg)*), &bt.to_string());
        }
        #[cfg(not(target_os = "ios"))]
        crate::boot_logging::boot_log(&format!($($arg)*));
    };
}

/// Appends a launch line to `Documents/horizon-boot.log` on iOS.
#[cfg(target_os = "ios")]
pub fn boot_log(stage: &str) {
    use std::io::Write as _;
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::PathBuf::from(home).join("Documents");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("horizon-boot.log"))
    else {
        return;
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let _ = writeln!(file, "{ms} [{thread_name}] {stage}");
    let _ = file.flush();
}

/// Enhanced boot log with file/line context.
#[cfg(target_os = "ios")]
pub fn boot_log_detailed(stage: &str, file: &str, line: u32) {
    use std::io::Write as _;
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::PathBuf::from(home).join("Documents");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file_handle) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("horizon-boot.log"))
    else {
        return;
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let _ = writeln!(file_handle, "{ms} [{thread_name}] {stage} @ {file}:{line}");
    let _ = file_handle.flush();
}

/// Boot log with captured backtrace (for error checkpoints).
#[cfg(target_os = "ios")]
pub fn boot_log_with_backtrace(stage: &str, backtrace: &str) {
    use std::io::Write as _;
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::PathBuf::from(home).join("Documents");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("horizon-boot.log"))
    else {
        return;
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    let _ = writeln!(file, "{ms} [{thread_name}] {stage}");
    for line in backtrace.lines() {
        let _ = writeln!(file, "{ms} [{thread_name}]   {line}");
    }
    let _ = file.flush();
}

#[cfg(not(target_os = "ios"))]
pub fn boot_log(_stage: &str) {}

#[cfg(not(target_os = "ios"))]
pub fn boot_log_detailed(_stage: &str, _file: &str, _line: u32) {}

#[cfg(not(target_os = "ios"))]
pub fn boot_log_with_backtrace(_stage: &str, _backtrace: &str) {}