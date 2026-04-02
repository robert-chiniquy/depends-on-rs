use std::net::TcpStream;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::config::WaitFor;

pub fn wait_for_ready(
    name: &str,
    wait_for: &WaitFor,
    log_rx: &Receiver<String>,
    child: &mut std::process::Child,
) -> Result<()> {
    let timeout = wait_for
        .timeout_duration()
        .unwrap_or_else(|| Duration::from_secs(30));
    let deadline = Instant::now() + timeout;

    if let Some(port) = wait_for.port {
        wait_for_port(name, port, deadline)
    } else if let Some(code) = wait_for.exit_code {
        wait_for_exit(name, code, child, deadline)
    } else if let Some(pattern) = &wait_for.log_pattern {
        wait_for_log(name, pattern, log_rx, deadline)
    } else {
        Ok(())
    }
}

fn wait_for_port(name: &str, port: u16, deadline: Instant) -> Result<()> {
    let mut backoff = Duration::from_millis(50);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_millis(500));
    }
    bail!("target {name:?}: timed out waiting for port {port}")
}

fn wait_for_exit(
    name: &str,
    expected: i32,
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<()> {
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => match status.code() {
                Some(code) if code == expected => return Ok(()),
                Some(code) => {
                    bail!("target {name:?}: exited with code {code}, expected {expected}")
                }
                None => bail!("target {name:?}: exited without a code"),
            },
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => bail!("target {name:?}: failed while waiting for exit: {error}"),
        }
    }
    bail!("target {name:?}: timed out waiting for exit")
}

fn wait_for_log(
    name: &str,
    pattern: &str,
    log_rx: &Receiver<String>,
    deadline: Instant,
) -> Result<()> {
    let re = Regex::new(pattern).with_context(|| format!("invalid log pattern {pattern:?}"))?;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match log_rx.recv_timeout(remaining) {
            Ok(line) => {
                if re.is_match(&line) {
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => {
                bail!("target {name:?}: log stream closed before readiness pattern matched")
            }
        }
    }
    bail!("target {name:?}: timed out waiting for log pattern {pattern:?}")
}
