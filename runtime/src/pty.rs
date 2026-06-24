//! Capsule Runtime - PTY management
//!
//! Handles PTY creation, I/O, and lifecycle using portable-pty.
//! Supports both local shell and Docker container exec.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Error, Debug)]
pub enum PtyError {
    #[error("Failed to open PTY: {0}")]
    OpenFailed(String),
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),
    #[error("PTY I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug)]
pub enum PtyCommand {
    Write(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Shutdown,
}

/// Spawn a PTY with a local bash shell (for testing)
pub fn spawn_local_pty(
    cols: u16,
    rows: u16,
) -> Result<(mpsc::Sender<PtyCommand>, mpsc::Receiver<Vec<u8>>), PtyError> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<PtyCommand>(64);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);

    std::thread::spawn(move || {
        if let Err(e) = run_local_pty_thread(cols, rows, cmd_rx, output_tx) {
            error!("PTY thread error: {}", e);
        }
    });

    Ok((cmd_tx, output_rx))
}

/// Spawn a PTY that executes into a Docker container
pub fn spawn_docker_pty(
    container_id: String,
    cols: u16,
    rows: u16,
) -> Result<(mpsc::Sender<PtyCommand>, mpsc::Receiver<Vec<u8>>), PtyError> {
    spawn_docker_pty_with_env(container_id, cols, rows, &[])
}

/// Spawn a PTY that executes into a Docker container with extra env vars
pub fn spawn_docker_pty_with_env(
    container_id: String,
    cols: u16,
    rows: u16,
    env_vars: &[(String, String)],
) -> Result<(mpsc::Sender<PtyCommand>, mpsc::Receiver<Vec<u8>>), PtyError> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<PtyCommand>(64);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);

    let env_owned: Vec<(String, String)> = env_vars.to_vec();

    std::thread::spawn(move || {
        if let Err(e) =
            run_docker_pty_thread(&container_id, cols, rows, &env_owned, cmd_rx, output_tx)
        {
            error!("Docker PTY thread error: {}", e);
        }
    });

    Ok((cmd_tx, output_rx))
}

// ── Local PTY ──────────────────────────────────────────────────────────────────

fn run_local_pty_thread(
    cols: u16,
    rows: u16,
    cmd_rx: mpsc::Receiver<PtyCommand>,
    output_tx: mpsc::Sender<Vec<u8>>,
) -> Result<(), PtyError> {
    let mut cmd = CommandBuilder::new("/bin/bash");
    cmd.args(["--norc", "--noprofile"]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("PS1", "$ ");

    run_pty_thread(cols, rows, cmd, cmd_rx, output_tx)
}

// ── Docker PTY ─────────────────────────────────────────────────────────────────

fn run_docker_pty_thread(
    container_id: &str,
    cols: u16,
    rows: u16,
    extra_env: &[(String, String)],
    cmd_rx: mpsc::Receiver<PtyCommand>,
    output_tx: mpsc::Sender<Vec<u8>>,
) -> Result<(), PtyError> {
    let mut cmd = CommandBuilder::new("docker");
    let mut args: Vec<&str> = vec!["exec", "-it", "-e", "TERM=xterm-256color", "-e", "LANG=C.UTF-8"];

    let env_strings: Vec<String> = extra_env
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();

    for env_str in &env_strings {
        args.push("-e");
        args.push(env_str);
    }

    args.extend_from_slice(&["-w", "/workspace", container_id, "/bin/bash"]);
    cmd.args(args);

    run_pty_thread(cols, rows, cmd, cmd_rx, output_tx)
}

// ── Common PTY thread ──────────────────────────────────────────────────────────

fn run_pty_thread(
    cols: u16,
    rows: u16,
    cmd: CommandBuilder,
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

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| PtyError::SpawnFailed(e.to_string()))?;

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

    info!(cols, rows, "PTY spawned");

    let reader_handle = std::thread::spawn(move || run_reader_thread(reader, output_tx));

    loop {
        match cmd_rx.blocking_recv() {
            Some(PtyCommand::Write(data)) => {
                if !write_to_pty(&mut writer, &data) {
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

    drop(writer);
    drop(master);
    let _ = reader_handle.join();

    match child.wait() {
        Ok(status) => info!("Process exited with status: {:?}", status),
        Err(e) => warn!("Failed to wait for process: {}", e),
    }

    info!("PTY thread exiting");
    Ok(())
}

fn write_to_pty(writer: &mut dyn Write, data: &[u8]) -> bool {
    if let Err(e) = writer.write_all(data) {
        error!("PTY write error: {}", e);
        return false;
    }
    if let Err(e) = writer.flush() {
        error!("PTY flush error: {}", e);
        return false;
    }
    true
}

fn run_reader_thread(mut reader: Box<dyn Read + Send>, output_tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0u8; 4096];
    loop {
        let Some(n) = read_pty_chunk(&mut reader, &mut buf) else { break };
        let data = buf[..n].to_vec();
        if output_tx.blocking_send(data).is_err() {
            warn!("Output channel closed");
            break;
        }
    }
}

fn read_pty_chunk(reader: &mut Box<dyn Read + Send>, buf: &mut [u8]) -> Option<usize> {
    match reader.read(buf) {
        Ok(0) => {
            info!("PTY EOF");
            None
        }
        Ok(n) => Some(n),
        Err(e) if e.kind() != std::io::ErrorKind::Other => {
            error!("PTY read error: {}", e);
            None
        }
        Err(_) => None,
    }
}
