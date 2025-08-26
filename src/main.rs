use different::{DiffSettings, line_diff};
use serde::Deserialize;
use std::fmt::Display;
use std::thread;
use std::time::Duration;
use std::{
    fs,
    io::Read,
    io::{Write, stdout},
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
};
use wait_timeout::ChildExt;

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use log::{debug, error, info, warn};
use std::process::Command;

const DEFAULT_SNAPSHOT_FILE_NAME: &str = "snapshot.toml";
const INDENT: &str = "    ";

fn exit_code_zero() -> i32 {
    0
}

#[derive(Debug, Parser)]
struct Cli {
    /// Exit on the first failed test without running any subsequent tests
    #[clap(short, long)]
    fail_fast: bool,

    /// Snapshot dir
    snapshots: PathBuf,
    // TODO: json output
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

#[derive(thiserror::Error, Debug)]
enum RunErr {
    #[error("spawn failed: {0}")]
    Spawn(std::io::Error),
    #[error("io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("utf8 stdout: {0}")]
    Utf8Out(#[from] std::string::FromUtf8Error),
    #[error("utf8 stderr: {0}")]
    Utf8Err(std::string::FromUtf8Error),
}

#[derive(Debug)]
struct Output {
    stdout: String,
    stderr: String,
    code: ExitStatus,
}

fn run_cmd(
    command: &Path,
    args: &[String],
    timeout: Option<u64>,
    stdin: Option<&String>,
) -> Result<Output> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunErr::Spawn)?;
    if let Some(stdin) = stdin {
        let stdin_handle = child.stdin.as_mut().unwrap();
        stdin_handle.write_all(stdin.as_bytes())?;
    }

    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();

    let t_out = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let t_err = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });

    let status = if let Some(timeout) = timeout {
        child.wait_timeout(Duration::from_millis(timeout))?
    } else {
        Some(child.wait()?)
    };

    let output = match status {
        Some(code) => {
            let stdout = String::from_utf8(t_out.join().unwrap()).map_err(RunErr::Utf8Out)?;
            let stderr = String::from_utf8(t_err.join().unwrap()).map_err(RunErr::Utf8Err)?;
            Output {
                code,
                stdout,
                stderr,
            }
        }
        None => {
            // timed out
            let _ = child.kill();
            let _ = child.wait();
            // still join readers so we get whatever was produced. TODO: do we care?
            let stdout = String::from_utf8(t_out.join().unwrap()).map_err(RunErr::Utf8Out)?;
            let stderr = String::from_utf8(t_err.join().unwrap()).map_err(RunErr::Utf8Err)?;
            let output = Output {
                code: ExitStatus::from_raw(9 << 8), // "killed" synthetic status (Unix-y)
                stdout,
                stderr,
            };
            output
        }
    };
    Ok(output)
}

fn diff(left: String, right: String) -> String {
    let settings = DiffSettings::default()
        .names("expected".to_string(), "got".to_string())
        .max_line_number(30);
    let diff = line_diff(&left, &right, &settings);
    diff.to_string()
}

#[derive(Debug)]
enum FailureReason {
    CommandNotFound(String),
    CommandFailed(String, String),
    Mismatch(
        Option<(i32, i32)>,
        Option<(String, String)>,
        Option<(String, String)>,
    ),
}

impl Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound(cmd) => {
                write!(f, "command '{cmd}' not found")
            }
            Self::CommandFailed(cmd, reason) => {
                write!(f, "command '{cmd}' failed: {reason}")
            }
            Self::Mismatch(code, stdout, stderr) => {
                if let Some((expected, got)) = code {
                    writeln!(f, "exit code mismatch: expected {expected}, got {got}")?;
                }

                if let Some((expected, got)) = stdout {
                    // TODO: diff should take Deref to str. Need to patch
                    writeln!(
                        f,
                        "stdout mismatch:\n{}",
                        diff(expected.to_string(), got.to_string())
                    )?;
                }

                if let Some((expected, got)) = stderr {
                    writeln!(
                        f,
                        "stderr mismatch:\n{}",
                        diff(expected.to_string(), got.to_string())
                    )?;
                }

                Ok(())
            }
        }
    }
}

impl std::error::Error for FailureReason {}

#[derive(Debug)]
enum TestStatus {
    Pass,
    Skip,
    Fail { reason: FailureReason },
}

#[derive(Debug)]
struct TestResult<'a> {
    status: TestStatus,
    test: &'a Test,
}

impl<'a> TestResult<'a> {
    fn fail(test: &'a Test, reason: FailureReason) -> Self {
        Self {
            status: TestStatus::Fail { reason },
            test,
        }
    }

    fn pass(test: &'a Test) -> Self {
        Self {
            status: TestStatus::Pass,
            test,
        }
    }
}

fn run_test(test: &Test) -> Result<(), FailureReason> {
    let name = &test.name;
    debug!("Running {name}");

    let test_file = &test.test;
    let cmd = &test_file.command;
    let exec = which::which(cmd).map_err(|_| FailureReason::CommandNotFound(cmd.to_string()))?;

    let output = run_cmd(
        &exec,
        &test_file.args,
        test_file.timeout_ms,
        test_file.stdin.as_ref(),
    )
    .map_err(|e| FailureReason::CommandFailed(cmd.to_string(), e.to_string()))?;

    let mut exit_code_mismatch = None;
    let expected = test.test.code;
    let got = output.code.code().unwrap();
    if expected != got {
        exit_code_mismatch = Some((expected, got));
    }

    let mut stdout_mismatch = None;
    if let Some(expected) = &test.test.stdout {
        let got = output.stdout;
        if *expected != got {
            stdout_mismatch = Some((expected.to_string(), got));
        }
    }

    let mut stderr_mismatch = None;
    if let Some(expected) = &test.test.stderr {
        let got = output.stderr;
        if *expected != got {
            stderr_mismatch = Some((expected.to_string(), got));
        }
    }

    if exit_code_mismatch.is_some() || stdout_mismatch.is_some() || stderr_mismatch.is_some() {
        Err(FailureReason::Mismatch(
            exit_code_mismatch,
            stdout_mismatch,
            stderr_mismatch,
        ))
    } else {
        Ok(())
    }
}

fn run(tests: &[Test], fail_fast: bool) -> Vec<TestResult> {
    let mut results = Vec::new();
    for test in tests {
        match run_test(test) {
            Ok(()) => results.push(TestResult::pass(test)),
            Err(reason) => {
                results.push(TestResult::fail(test, reason));
                if fail_fast {
                    break;
                }
            }
        }
    }
    results
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

fn display_results(results: &[TestResult]) {
    for result in results {
        let name = &result.test.name;

        match &result.status {
            TestStatus::Pass => {
                println!("[PASS] {name}");
            }
            TestStatus::Skip => {
                println!("[SKIP] {name}");
            }
            TestStatus::Fail { reason } => {
                println!("[FAIL] {name}");
                // TODO: display diffs
                println!("{INDENT}{reason}");
            }
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();
    let fail_fast = args.fail_fast;
    let snapshots_dir = args.snapshots;
    let tests = discover(&snapshots_dir)?;
    let results = run(&tests, fail_fast);
    display_results(&results);
    Ok(())
}
