use std::process::ExitCode;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use clap::{Parser, Subcommand};
use depends_on_rs::Manager;

#[derive(Parser)]
#[command(name = "depends-on-rs")]
#[command(about = "Declarative process orchestration for integration tests")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Start {
        #[arg(long)]
        config: String,
        targets: Vec<String>,
    },
    Run {
        #[arg(long)]
        config: String,
        #[arg(long)]
        targets: Vec<String>,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Command::Start { config, targets } => {
            let manager = Manager::load(config)?;
            let handle = manager.start(&targets)?;
            let running = Arc::new(AtomicBool::new(true));
            let signal_flag = Arc::clone(&running);
            ctrlc::set_handler(move || {
                signal_flag.store(false, Ordering::SeqCst);
            })?;

            while running.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
            drop(handle);
            Ok(0)
        }
        Command::Run {
            config,
            targets,
            command,
        } => {
            let manager = Manager::load(config)?;
            manager.run_command(&targets, &command)
        }
    }
}
