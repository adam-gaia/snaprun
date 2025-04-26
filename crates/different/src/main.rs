use anyhow::Result;
use clap::Parser;
use different::{DiffSettings, line_diff};
use log::debug;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    /// Input file 1
    left: PathBuf,

    /// Input file 2
    right: PathBuf,

    #[clap(flatten)]
    settings: DiffSettings,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();
    let left = fs::read_to_string(args.left)?;
    let right = fs::read_to_string(args.right)?;
    let settings = args.settings;
    debug!("{settings:?}");
    line_diff(&left, &right, &settings, true);
    Ok(())
}
