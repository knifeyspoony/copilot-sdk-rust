// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

// Minimal test for debugging pipe issues

use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Debug Pipe Test ===\n");

    // Find node and npm-loader path
    let node_path = which::which("node")?;
    let npm_loader = std::path::PathBuf::from(std::env::var("APPDATA")?)
        .join("npm")
        .join("node_modules")
        .join("@github")
        .join("copilot")
        .join("npm-loader.js");

    println!("Node: {:?}", node_path);
    println!("Loader: {:?}", npm_loader);

    // Spawn process
    let mut child = Command::new(&node_path)
        .arg(&npm_loader)
        .args(["--server", "--log-level", "info", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    println!("Process spawned, pid: {:?}", child.id());

    // Take handles
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");

    // Wait a moment
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("Process still running: {:?}", child.try_wait()?.is_none());

    // Try to read stderr
    let mut stderr_buf = [0u8; 1024];
    match tokio::time::timeout(
        std::time::Duration::from_millis(100),
        stderr.read(&mut stderr_buf),
    )
    .await
    {
        Ok(Ok(n)) if n > 0 => {
            println!("Stderr: {}", String::from_utf8_lossy(&stderr_buf[..n]));
        }
        _ => println!("No stderr"),
    }

    // Build JSON-RPC message
    let msg = r#"{"jsonrpc":"2.0","method":"ping","params":{"message":null},"id":1}"#;
    let frame = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    println!("\nSending: {} bytes", frame.len());

    // Write
    match stdin.write_all(frame.as_bytes()).await {
        Ok(()) => println!("write_all: OK"),
        Err(e) => println!("write_all: ERROR - {:?}", e),
    }

    // Flush
    match stdin.flush().await {
        Ok(()) => println!("flush: OK"),
        Err(e) => println!("flush: ERROR - {:?}", e),
    }

    // Try to read response
    println!("\nWaiting for response...");
    let mut response_buf = [0u8; 4096];
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stdout.read(&mut response_buf),
    )
    .await
    {
        Ok(Ok(n)) => {
            println!(
                "Response ({} bytes): {}",
                n,
                String::from_utf8_lossy(&response_buf[..n])
            );
        }
        Ok(Err(e)) => println!("Read error: {:?}", e),
        Err(_) => println!("Timeout"),
    }

    // Check if process is still running
    println!("\nProcess still running: {:?}", child.try_wait()?.is_none());

    // Kill
    child.start_kill()?;
    println!("Done!");
    Ok(())
}
