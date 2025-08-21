use serde::Deserialize;
use std::{
    fs,
    io::Stdout,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use log::{debug, error, info, warn};
use snapbox::cmd::Command;

const DEFAULT_SNAPSHOT_FILE_NAME: &str = "snapshot.toml";

fn exit_code_zero() -> i32 {
    0
}

#[derive(Debug, Parser)]
struct Cli {
    /// Snapshot dir
    snapshots: PathBuf,
}

fn discover(dir: &Path) -> Result<Vec<Test>> {
    let mut tests = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(Some("toml")) = path.extension().map(|x| x.to_str()) {
                let test = Test::from_file(&path)?;
                tests.push(test);
            }
        } else if path.is_dir() {
            let test = Test::from_dir(&path)?;
            tests.push(test);
        } else {
            warn!("Unexpected file in snapshot dir {}", path.display());
        }
    }

    Ok(tests)
}

fn run_test(test: &Test) -> Result<()> {
    let name = &test.name;
    debug!("Running {name}");

    let test = &test.test;
    let exec = which::which(&test.command)?;

    let mut cmd = Command::new(exec).args(&test.args);
    if let Some(stdin) = &test.stdin {
        cmd = cmd.stdin(stdin);
    }
    if let Some(timeout_ms) = test.timeout_ms {
        cmd = cmd.timeout(std::time::Duration::from_millis(timeout_ms));
    }

    let mut foo = cmd.assert().code(test.code);

    if let Some(stdout) = &test.stdout {
        foo = foo.stdout_eq(stdout);
    }
    if let Some(stderr) = &test.stderr {
        foo = foo.stderr_eq(stderr);
    }

    info!("Success");

    Ok(())
}

fn run(tests: &[Test]) -> Result<()> {
    let mut res = Result::Ok(());
    for test in tests {
        let name = &test.name;
        let test_res = run_test(test);
        if test_res.is_err() {
            error!("Test {name} failed");
            res = Result::Err(anyhow!("Test failed"));
        }
    }

    res
}

#[derive(Debug, Deserialize)]
struct TestFile {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: Option<u64>,

    stdin: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,

    #[serde(default = "exit_code_zero")]
    code: i32,
}

#[derive(Debug)]
struct Test {
    name: String,
    test: TestFile,
}

impl Test {
    fn from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let test = toml::from_str(&contents)?;
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let test = Test { name, test };
        Ok(test)
    }

    fn from_dir(dir: &Path) -> Result<Self> {
        let snapshot_file = dir.join(DEFAULT_SNAPSHOT_FILE_NAME);
        let contents = fs::read_to_string(&snapshot_file)?;
        let base = toml::from_str(&contents)?;
        let mut builder = TestBuilder::with_base(base);

        let name = dir.file_stem().unwrap().to_str().unwrap().to_string();
        builder = builder.name(name);

        let stdin_file = dir.join("stdin.txt");
        if stdin_file.is_file() {
            let stdin = fs::read_to_string(&stdin_file)?;
            builder = builder.stdin(stdin)?;
        }

        let stdout_file = dir.join("stdout.txt");
        if stdout_file.is_file() {
            let stdout = fs::read_to_string(&stdout_file)?;
            builder = builder.stdout(stdout)?
        }

        let stderr_file = dir.join("stderr.txt");
        if stderr_file.is_file() {
            let stderr = fs::read_to_string(&stderr_file)?;
            builder = builder.stderr(stderr)?
        }

        let test = builder.build()?;
        Ok(test)
    }
}

#[derive(Debug)]
struct TestBuilder {
    name: Option<String>,
    command: String,
    args: Vec<String>,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    code: Option<i32>,
}

impl TestBuilder {
    fn with_base(base: TestFile) -> Self {
        Self {
            name: None,
            command: base.command,
            args: base.args,
            timeout_ms: base.timeout_ms,
            stdin: base.stdin,
            stdout: base.stdout,
            stderr: base.stderr,
            code: Some(base.code),
        }
    }

    fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    fn stdin(mut self, stdin: String) -> Result<Self> {
        if self.stdin.is_some() {
            bail!("Stdin is already defined");
        }
        self.stdin = Some(stdin);
        Ok(self)
    }

    fn stdout(mut self, stdout: String) -> Result<Self> {
        if self.stdout.is_some() {
            bail!("Stdout is already defined");
        }
        self.stdout = Some(stdout);
        Ok(self)
    }

    fn stderr(mut self, stderr: String) -> Result<Self> {
        if self.stderr.is_some() {
            bail!("Stderr is already defined");
        }
        self.stderr = Some(stderr);
        Ok(self)
    }

    fn build(self) -> Result<Test> {
        let command = self.command;
        let args = self.args;
        let timeout_ms = self.timeout_ms;
        let stdin = self.stdin;
        let stdout = self.stdout;
        let stderr = self.stderr;
        let code = self.code.unwrap_or(exit_code_zero());
        let test = TestFile {
            command,
            args,
            timeout_ms,
            stdin,
            stdout,
            stderr,
            code,
        };

        let name = self.name.ok_or(anyhow!("Name not set"))?;

        Ok(Test { name, test })
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();

    let snapshots_dir = args.snapshots;

    let tests = discover(&snapshots_dir)?;

    run(&tests)?;

    Ok(())
}
