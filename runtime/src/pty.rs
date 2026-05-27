//! Capsule Runtime - PTY management
//!
//! Handles PTY creation, I/O, and lifecycle using portable-pty.
//! All PTY operations run in a dedicated thread to avoid Send/Sync issues.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Error, Debug)]
pub enum PtyError {
    #[error("Failed to open PTY: {0}")]
    OpenFailed(String),
    #[error("Failed to spawn shell: {0}")]
    SpawnFailed(String),
    #[error("PTY I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("PTY not found")]
    NotFound,
}

/// Commands that can be sent to a running PTY
#[derive(Debug)]
pub enum PtyCommand {
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Shutdown,
}

/// Spawn a PTY and return channels for communication.
/// All PTY operations happen in a dedicated OS thread.
pub fn spawn_pty(
    cols: u16,
    rows: u16,
) -> Result<(mpsc::Sender<PtyCommand>, mpsc::Receiver<Vec<u8>>), PtyError> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<PtyCommand>(64);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);

    // Spawn a dedicated thread for all PTY operations
    std::thread::spawn(move || {
        if let Err(e) = run_pty_thread(cols, rows, cmd_rx, output_tx) {
            error!("PTY thread error: {}", e);
        }
    });

    Ok((cmd_tx, output_rx))
}

/// Main PTY thread function - handles all PTY I/O in a single thread
fn run_pty_thread(
    cols: u16,
    rows: u16,
    mut cmd_rx: mpsc::Receiver<PtyCommand>,
    output_tx: mpsc::Sender<Vec<u8>>,
) -> Result<(), PtyError> {
    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| PtyError::OpenFailed(e.to_string()))?;

    // Use bash with minimal config for debugging
    let mut cmd = CommandBuilder::new("/bin/bash");
    cmd.args(["--norc", "--noprofile"]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("PS1", "$ ");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| PtyError::SpawnFailed(e.to_string()))?;

    // Drop slave - we only need the master now
    drop(pair.slave);

    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| PtyError::IoError(std::io::Error::other(e.to_string())))?;

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| PtyError::IoError(std::io::Error::other(e.to_string())))?;

    let master = pair.master;

    info!(cols, rows, "PTY spawned with bash");

    // Spawn a thread to read from PTY and send to output channel
    let reader_handle = std::thread::spawn(move || run_reader_thread(reader, output_tx));

    // Main loop: handle commands
    loop {
        // Use blocking_recv since we're in a dedicated thread
        match cmd_rx.blocking_recv() {
            Some(PtyCommand::Write(data)) => {
                if let Err(e) = writer.write_all(&data) {
                    error!("PTY write error: {}", e);
                    break;
                }
                if let Err(e) = writer.flush() {
                    error!("PTY flush error: {}", e);
                    break;
                }
            }
            Some(PtyCommand::Resize { cols, rows }) => {
                info!(cols, rows, "PTY resize");
                if let Err(e) = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                }) {
                    error!("PTY resize error: {}", e);
                }
            }
            Some(PtyCommand::Shutdown) => {
                info!("PTY shutdown requested");
                break;
            }
            None => {
                info!("Command channel closed");
                break;
            }
        }
    }

    // Clean up
    drop(writer);
    drop(master);

    // Wait for reader thread
    let _ = reader_handle.join();

    // Wait for child process
    match child.wait() {
        Ok(status) => info!("Shell exited with status: {:?}", status),
        Err(e) => warn!("Failed to wait for shell: {}", e),
    }

    info!("PTY thread exiting");
    Ok(())
}

/// PTY reader thread — reads output from the PTY and forwards it to the output channel.
fn run_reader_thread(mut reader: Box<dyn Read + Send>, output_tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0u8; 4096];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                info!("PTY EOF");
                break;
            }
            Ok(n) => n,
            Err(e) => {
                // Non-Other errors are unexpected; log them
                if e.kind() != std::io::ErrorKind::Other {
                    error!("PTY read error: {}", e);
                }
                break;
            }
        };
        let data = buf[..n].to_vec();
        if output_tx.blocking_send(data).is_err() {
            warn!("Output channel closed");
            break;
        }
    }
}
