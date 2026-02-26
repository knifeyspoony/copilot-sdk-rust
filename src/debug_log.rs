// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

//! Optional file-based debug logger for the Copilot SDK.
//!
//! Call `init(path)` to enable logging. If `init` is never called, `write()`
//! and `sdk_dlog!()` are silent no-ops (no file I/O, no allocation).
//!
//! This module has no external dependencies — only `std`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_FILE: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Open (or create) a log file. Subsequent `write()` calls append to it.
/// Safe to call multiple times — only the first call takes effect.
pub fn init(path: &str) {
    if let Some(parent) = Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(file) => {
            let _ = LOG_FILE.set(Mutex::new(BufWriter::new(file)));
            write(&format!("=== copilot-sdk debug log: {path} ==="));
        }
        Err(e) => eprintln!("copilot-sdk debug_log: failed to open {path}: {e}"),
    }
}

/// Append a timestamped line. No-op if `init()` was never called.
pub fn write(msg: &str) {
    let Some(lock) = LOG_FILE.get() else { return };
    let Ok(mut w) = lock.lock() else { return };
    let ts = now_ms();
    let _ = writeln!(w, "[{ts}] {msg}");
    let _ = w.flush();
}

/// Write a formatted message to the SDK debug log.
/// No-op if `init()` was never called.
#[macro_export]
macro_rules! sdk_dlog {
    ($($arg:tt)*) => {
        $crate::debug_log::write(&format!($($arg)*))
    };
}
