use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};

use anyhow::{Context, Result, bail};
use os_pipe::{PipeReader, PipeWriter};

use crate::config::{Config, Target};
use crate::envsubst;
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct RunningProcess {
    pub child: Child,
    pub pgid: i32,
    pub log_rx: mpsc::Receiver<String>,
}

#[derive(Default)]
pub struct PipeRegistry {
    stdin_sources: BTreeMap<String, PipeReader>,
    stream_subscribers: BTreeMap<(String, StreamKind), Vec<PipeWriter>>,
}

impl PipeRegistry {
    pub fn build(config: &Config, targets: &[String]) -> Result<Self> {
        let mut registry = PipeRegistry::default();
        for name in targets {
            let target = &config[name];
            if let Some(spec) = target.fds.get("stdin") {
                if let Some(source) = spec.strip_prefix("pipe:") {
                    let (source_target, stream_name) = source
                        .split_once('.')
                        .ok_or_else(|| anyhow::anyhow!("invalid pipe source {spec:?}"))?;
                    let stream = match stream_name {
                        "stdout" => StreamKind::Stdout,
                        "stderr" => StreamKind::Stderr,
                        other => bail!("unsupported pipe source stream {other:?}"),
                    };
                    let (reader, writer) = os_pipe::pipe()?;
                    registry.stdin_sources.insert(name.clone(), reader);
                    registry
                        .stream_subscribers
                        .entry((source_target.to_string(), stream))
                        .or_default()
                        .push(writer);
                }
            }
        }
        Ok(registry)
    }

    pub fn stdin_reader(&mut self, target: &str) -> Option<PipeReader> {
        self.stdin_sources.remove(target)
    }

    pub fn subscribers(&mut self, target: &str, stream: StreamKind) -> Vec<PipeWriter> {
        self.stream_subscribers
            .remove(&(target.to_string(), stream))
            .unwrap_or_default()
    }
}

enum OutputSink {
    InheritStdout,
    InheritStderr,
    Null,
    File(Arc<Mutex<File>>),
}

impl OutputSink {
    fn write_all(&self, bytes: &[u8]) {
        match self {
            OutputSink::InheritStdout => {
                let _ = std::io::stdout().write_all(bytes);
                let _ = std::io::stdout().flush();
            }
            OutputSink::InheritStderr => {
                let _ = std::io::stderr().write_all(bytes);
                let _ = std::io::stderr().flush();
            }
            OutputSink::Null => {}
            OutputSink::File(file) => {
                if let Ok(mut guard) = file.lock() {
                    let _ = guard.write_all(bytes);
                    let _ = guard.flush();
                }
            }
        }
    }
}

fn output_sink(spec: Option<&String>, fallback: OutputSink, base_dir: &Path) -> Result<OutputSink> {
    match spec.map(String::as_str) {
        None | Some("inherit") => Ok(fallback),
        Some("null") => Ok(OutputSink::Null),
        Some(value) if value.starts_with("file:") => {
            let path = base_dir.join(value.trim_start_matches("file:"));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Ok(OutputSink::File(Arc::new(Mutex::new(File::create(path)?))))
        }
        Some(value) => bail!("unsupported output sink {value:?}"),
    }
}

fn stdin_stdio(
    name: &str,
    target: &Target,
    pipes: &mut PipeRegistry,
    base_dir: &Path,
) -> Result<Stdio> {
    if let Some(spec) = target.fds.get("stdin") {
        if spec == "inherit" {
            return Ok(Stdio::inherit());
        }
        if spec == "null" {
            return Ok(Stdio::null());
        }
        if let Some(path) = spec.strip_prefix("file:") {
            return Ok(Stdio::from(File::open(base_dir.join(path))?));
        }
        if spec.starts_with("pipe:") {
            let reader = pipes
                .stdin_reader(name)
                .ok_or_else(|| anyhow::anyhow!("missing pipe reader for target {name:?}"))?;
            return Ok(Stdio::from(reader));
        }
        bail!("unsupported stdin fd spec {spec:?}");
    }
    Ok(Stdio::inherit())
}

fn output_spec(target: &Target, key: &str, legacy: &Option<String>) -> Option<String> {
    target
        .fds
        .get(key)
        .cloned()
        .or_else(|| legacy.as_ref().map(|value| format!("file:{value}")))
}

pub fn spawn_process(
    name: &str,
    target: &Target,
    base_dir: &Path,
    pipes: &mut PipeRegistry,
) -> Result<RunningProcess> {
    let mut cmd = Command::new(
        target
            .cmd
            .first()
            .ok_or_else(|| anyhow::anyhow!("target {name:?}: empty cmd"))?,
    );
    for arg in target.cmd.iter().skip(1) {
        cmd.arg(envsubst::expand(arg));
    }

    let cwd = target
        .cwd
        .as_ref()
        .map(|value| base_dir.join(envsubst::expand(value)))
        .unwrap_or_else(|| base_dir.to_path_buf());
    cmd.current_dir(cwd);

    for (key, value) in &target.env {
        cmd.env(key, envsubst::expand(value));
    }

    cmd.stdin(stdin_stdio(name, target, pipes, base_dir)?);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning target {name:?}"))?;
    let pgid = child.id() as i32;

    let stdout_sink = output_sink(
        output_spec(target, "stdout", &target.stdout).as_ref(),
        OutputSink::InheritStdout,
        base_dir,
    )?;
    let stderr_sink = output_sink(
        output_spec(target, "stderr", &target.stderr).as_ref(),
        OutputSink::InheritStderr,
        base_dir,
    )?;

    let stdout_subscribers = pipes.subscribers(name, StreamKind::Stdout);
    let stderr_subscribers = pipes.subscribers(name, StreamKind::Stderr);

    let (log_tx, log_rx) = mpsc::channel();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stdout for target {name:?}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stderr for target {name:?}"))?;

    mirror_stream(stdout, stdout_sink, stdout_subscribers, log_tx.clone());
    mirror_stream(stderr, stderr_sink, stderr_subscribers, log_tx);

    Ok(RunningProcess {
        child,
        pgid,
        log_rx,
    })
}

fn mirror_stream<R: Read + Send + 'static>(
    reader: R,
    sink: OutputSink,
    mut subscribers: Vec<PipeWriter>,
    log_tx: mpsc::Sender<String>,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = [0u8; 8192];
        let mut line_buf = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    let bytes = &buf[..count];
                    sink.write_all(bytes);
                    subscribers.retain_mut(|writer| writer.write_all(bytes).is_ok());
                    line_buf.extend_from_slice(bytes);
                    while let Some(pos) = line_buf.iter().position(|byte| *byte == b'\n') {
                        let line = String::from_utf8_lossy(&line_buf[..pos]).to_string();
                        let _ = log_tx.send(line);
                        line_buf.drain(..=pos);
                    }
                }
                Err(_) => break,
            }
        }
        if !line_buf.is_empty() {
            let _ = log_tx.send(String::from_utf8_lossy(&line_buf).to_string());
        }
    });
}

pub fn signal_number(name: Option<&str>) -> i32 {
    match name.unwrap_or("SIGTERM") {
        "SIGINT" => libc::SIGINT,
        "SIGKILL" => libc::SIGKILL,
        _ => libc::SIGTERM,
    }
}
