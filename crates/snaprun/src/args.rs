use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand, Clone)]
pub enum Command {
    New {
        /// Name of snapshot
        name: String,

        /// Command to run
        #[clap(short, long, required = true)]
        command: String,

        /// Timeout (ms)
        #[clap(short, long)]
        timeout: Option<u64>,

        /// Stdin to provide command
        #[clap(long)]
        stdin: Option<String>,

        /// Output snapshot dir
        #[clap(short, long)]
        outdir: PathBuf,
    },
    Update {
        /// Name of snapshot to update
        name: String,

        /// New name for snapshot
        #[clap(short, long)]
        new_name: Option<String>,

        /// Update the command that is ran
        #[clap(short, long)]
        command: String,

        /// Update timeout (ms)
        #[clap(short, long)]
        timeout: Option<u64>,

        /// Update stdin provided to command
        #[clap(long)]
        stdin: Option<String>,

        /// Output snapshot dir
        #[clap(short, long)]
        outdir: PathBuf,
    },
    Run {
        /// Exit on the first failed test without running any subsequent tests
        #[clap(short, long)]
        fail_fast: bool,

        snapshots: Option<PathBuf>,
    },
}

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>, // TODO: json output
}

impl Cli {
    pub fn command(&self) -> Command {
        self.command.clone().unwrap_or_else(|| Command::Run {
            fail_fast: false,
            snapshots: None,
        })
    }
}
