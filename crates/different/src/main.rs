use anyhow::Result;
use clap::Parser;
use different::{DiffSettings, line_diff};
use log::debug;
use pathdiff::diff_paths;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::path::PathBuf;
use std::{env, fs};

#[derive(Parser)]
struct Cli {
    /// Input file 1
    left: PathBuf,

    /// Input file 2
    right: PathBuf,

    #[clap(flatten)]
    settings: DiffSettings,
}

fn display_name(path: &Path, cwd: &Path) -> String {
    diff_paths(&path, &cwd)
        .map(|p| format!("./{}", p.display()))
        .unwrap_or(path.display().to_string())
}

/// Returns (Name: String, contents: String, num_lines: usize)
fn process_file(path: &Path, cwd: &Path) -> Result<(String, String, usize)> {
    let path = path.canonicalize()?;
    let name = display_name(&path, cwd);
    let contents = fs::read_to_string(path)?;
    let num_lines = contents.lines().count();
    Ok((name, contents, num_lines))
}

fn main() -> Result<()> {
    env_logger::init();
    let cwd = env::current_dir()?;
    let args = Cli::parse();

    let left = PathBuf::from(args.left);
    let right = PathBuf::from(args.right);

    let (left_name, left_contents, left_num_lines) = process_file(&left, &cwd)?;
    let (right_name, right_contents, right_num_lines) = process_file(&right, &cwd)?;

    let num_lines = std::cmp::max(left_num_lines, right_num_lines);
    let settings = args
        .settings
        .names(left_name, right_name)
        .max_line_number(num_lines);
    debug!("{settings:?}");

    let diff = line_diff(&left_contents, &right_contents, &settings);
    println!("{diff}");

    Ok(())
}
