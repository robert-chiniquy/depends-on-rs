use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::Config;
use crate::dag::{topological_order, validate_config};
use crate::envsubst;
use crate::process::{PipeRegistry, RunningProcess, signal_number, spawn_process};
use crate::ready::wait_for_ready;

#[derive(Debug)]
pub struct Manager {
    config: Config,
    base_dir: PathBuf,
}

impl Manager {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let mut manager = Self::parse(&data)?;
        manager.base_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok(manager)
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut config: Config = serde_json::from_slice(data).context("parsing config JSON")?;
        for target in config.values_mut() {
            target.cmd = target
                .cmd
                .iter()
                .map(|value| envsubst::expand(value))
                .collect();
            target.cwd = target.cwd.as_ref().map(|value| envsubst::expand(value));
            target.env = target
                .env
                .iter()
                .map(|(key, value)| (key.clone(), envsubst::expand(value)))
                .collect();
        }
        validate_config(&config)?;
        Ok(Self {
            config,
            base_dir: std::env::current_dir().context("getting current directory")?,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn start(&self, targets: &[String]) -> Result<RunHandle> {
        let order = topological_order(&self.config, targets)?;
        let mut pipes = PipeRegistry::build(&self.config, &order)?;
        let mut processes = BTreeMap::new();

        for name in order {
            let target = &self.config[&name];
            let mut process = spawn_process(&name, target, &self.base_dir, &mut pipes)
                .with_context(|| format!("starting target {name:?}"))?;
            wait_for_ready(&name, &target.wait_for, &process.log_rx, &mut process.child)
                .with_context(|| format!("waiting for target {name:?}"))?;
            processes.insert(name, process);
        }

        Ok(RunHandle {
            config: self.config.clone(),
            processes,
            stopped: false,
        })
    }

    pub fn run_command(&self, targets: &[String], command: &[String]) -> Result<i32> {
        let handle = self.start(targets)?;
        let status = std::process::Command::new(
            command
                .first()
                .ok_or_else(|| anyhow::anyhow!("run_command requires a command"))?,
        )
        .args(&command[1..])
        .status()
        .context("running follow-up command")?;
        drop(handle);
        Ok(status.code().unwrap_or(1))
    }
}

pub struct RunHandle {
    config: Config,
    processes: BTreeMap<String, RunningProcess>,
    stopped: bool,
}

impl RunHandle {
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;

        for (name, process) in &self.processes {
            let signal = signal_number(self.config[name].signal.as_deref());
            unsafe {
                libc::kill(-process.pgid, signal);
            }
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let mut all_done = true;
            for process in self.processes.values_mut() {
                match process.child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) => all_done = false,
                    Err(_) => {}
                }
            }
            if all_done {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        for process in self.processes.values() {
            unsafe {
                libc::kill(-process.pgid, libc::SIGKILL);
            }
        }
        for process in self.processes.values_mut() {
            let _ = process.child.wait();
        }
    }
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        self.stop();
    }
}
