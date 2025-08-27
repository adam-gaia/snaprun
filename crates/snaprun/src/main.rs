use different::{DiffSettings, line_diff};
use serde::{Deserialize, Serialize};
use snapbox::ToDebug;
use std::fmt::{Debug, Display};
use std::fs::File;
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

mod args;
use args::Cli;

const SNAPSHOT_FILE_NAME: &str = "snapshot.toml";
const STDOUT_FILE_NAME: &str = "stdout.txt";
const STDERR_FILE_NAME: &str = "stderr.txt";
const STDIN_FILE_NAME: &str = "stdin.txt";

const INDENT: &str = "  ";

fn exit_code_zero() -> i32 {
    0
}

fn discover(dir: PathBuf) -> Result<Vec<Test>> {
    let mut tests = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(Some("toml")) = path.extension().map(|x| x.to_str()) {
                let test = Test::from_file(path)?;
                tests.push(test);
            }
        } else if path.is_dir() {
            let test = Test::from_dir(path)?;
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

#[derive(Debug, Deserialize, Serialize, Clone)]
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
    path: PathBuf,
    test: TestFile,
}

impl Test {
    fn from_file(path: PathBuf) -> Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let test = toml::from_str(&contents)?;
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let test = Test { name, path, test };
        Ok(test)
    }

    fn from_dir(dir: PathBuf) -> Result<Self> {
        let snapshot_file = dir.join(SNAPSHOT_FILE_NAME);
        if !snapshot_file.is_file() {
            bail!("Snapshot file '{}' does not exist", snapshot_file.display());
        }
        let contents = fs::read_to_string(&snapshot_file)?;
        let base = toml::from_str(&contents)?;
        let mut builder = TestBuilder::with_base(base, dir.clone());

        let name = dir.file_stem().unwrap().to_str().unwrap().to_string();
        builder = builder.name(name);

        let stdin_file = dir.join(STDIN_FILE_NAME);
        if stdin_file.is_file() {
            let stdin = fs::read_to_string(&stdin_file)?;
            builder = builder.stdin(Some(stdin))?;
        }

        let stdout_file = dir.join(STDOUT_FILE_NAME);
        if stdout_file.is_file() {
            let stdout = fs::read_to_string(&stdout_file)?;
            builder = builder.stdout(stdout)?
        }

        let stderr_file = dir.join(STDERR_FILE_NAME);
        if stderr_file.is_file() {
            let stderr = fs::read_to_string(&stderr_file)?;
            builder = builder.stderr(stderr)?
        }

        let test = builder.build()?;
        Ok(test)
    }

    fn save(&self) -> Result<()> {
        let mut test = self.test.clone();
        let stdout = test.stdout.as_ref();
        let stderr = test.stderr.as_ref();
        let stdin = test.stdin.as_ref();

        // Save the streams to separate files (instead of to the toml)
        save_streams(&self.path, stdout, stderr, stdin)?;
        test.stdout = None;
        test.stderr = None;
        test.stdin = None;

        // Save the toml without the streams
        let path = &self.path.join(SNAPSHOT_FILE_NAME);
        let contents = toml::to_string(&test)?;
        let mut f = File::create(&path)?;
        write!(f, "{contents}")?;

        Ok(())
    }

    fn list_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.path.join(SNAPSHOT_FILE_NAME)];

        if self.test.stdout.is_some() {
            files.push(self.path.join(STDOUT_FILE_NAME));
        }

        if self.test.stderr.is_some() {
            files.push(self.path.join(STDERR_FILE_NAME));
        }

        if self.test.stdin.is_some() {
            files.push(self.path.join(STDIN_FILE_NAME));
        }

        files
    }
}

#[derive(Debug)]
struct SnapshotDiff {
    old: Test,
    new: Test,
}

impl SnapshotDiff {
    fn new(old: Test, new: Test) -> Self {
        Self { old, new }
    }

    fn render(&self) -> String {
        let mut s = String::new();
        foo(
            &mut s,
            "name",
            Some(&self.old.name),
            Some(&self.new.name),
            true,
        );
        foo(
            &mut s,
            "command",
            Some(&self.old.test.command),
            Some(&self.new.test.command),
            true,
        );
        foo(
            &mut s,
            "args",
            Some(&self.old.test.args.to_debug().to_string()),
            Some(&self.new.test.args.to_debug().to_string()),
            true,
        );
        foo(
            &mut s,
            "code",
            Some(self.old.test.code),
            Some(self.new.test.code),
            true,
        );
        foo(
            &mut s,
            "timeout",
            self.old.test.timeout_ms,
            self.new.test.timeout_ms,
            true,
        );
        foo(
            &mut s,
            "stdin",
            self.old.test.stdin.as_ref(),
            self.new.test.stdin.as_ref(),
            true,
        );
        foo(
            &mut s,
            "stdout",
            self.old.test.stdout.as_ref(),
            self.new.test.stdout.as_ref(),
            true,
        );
        foo(
            &mut s,
            "stderr",
            self.old.test.stderr.as_ref(),
            self.new.test.stderr.as_ref(),
            false,
        );

        s
    }
}

fn replace_newlines_shorten<T>(input: &T) -> String
where
    T: Eq + PartialEq + Display,
{
    let input = input.to_string();
    let len = input.len();
    let output = if len > 30 {
        let first = &input[0..5];
        let last_idx = len - 1;
        let x = last_idx - 5;
        let last = &input[x..];
        format!("{first}...{last}")
    } else {
        input
    };

    let output = output.replace("\n", "[NEWLINE]");
    output
}

fn foo<T>(s: &mut String, name: &str, old: Option<T>, new: Option<T>, newline: bool) -> ()
where
    T: Eq + PartialEq + Display,
{
    match (old, new) {
        (Some(old), Some(new)) => {
            if old == new {
                return;
            }

            let old = replace_newlines_shorten(&old);
            let new = replace_newlines_shorten(&new);
            s.push_str(&format!("{INDENT}{name}: '{old}' -> '{new}'"));
        }
        (Some(old), None) => {
            let old = replace_newlines_shorten(&old);
            s.push_str(&format!("{INDENT}unset {name} = '{old}'"));
        }
        (None, Some(new)) => {
            let new = replace_newlines_shorten(&new);
            s.push_str(&format!("{INDENT}set {name} = '{new}'"));
        }
        (None, None) => {
            return;
        }
    }

    if newline {
        s.push_str("\n")
    }
}

#[derive(Debug)]
struct TestBuilder {
    dir: PathBuf,
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
    fn new(command: String, dir: PathBuf) -> Self {
        Self {
            dir,
            name: None,
            command,
            args: Vec::new(),
            timeout_ms: None,
            stdin: None,
            stdout: None,
            stderr: None,
            code: None,
        }
    }

    fn with_base(base: TestFile, dir: PathBuf) -> Self {
        Self {
            dir,
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

    fn stdin(mut self, stdin: Option<String>) -> Result<Self> {
        if self.stdin.is_some() {
            bail!("Stdin is already defined");
        }
        self.stdin = stdin;
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

    fn timeout(mut self, timeout: Option<u64>) -> Self {
        self.timeout_ms = timeout;
        self
    }

    fn build(self) -> Result<Test> {
        let path = self.dir;
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

        Ok(Test { name, path, test })
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

fn discover_snapshots() -> Result<PathBuf> {
    todo!();
}

fn write_contents(path: &Path, contents: &str) -> Result<()> {
    let mut f = File::create(&path)?;
    write!(f, "{contents}")?;
    Ok(())
}

fn save_streams(
    dir: &Path,
    stdout: Option<&String>,
    stderr: Option<&String>,
    stdin: Option<&String>,
) -> Result<()> {
    if let Some(stdout) = stdout {
        let stdout_file = dir.join(STDOUT_FILE_NAME);
        write_contents(&stdout_file, stdout)?;
    }

    if let Some(stderr) = stderr {
        let stderr_file = dir.join(STDERR_FILE_NAME);
        write_contents(&stderr_file, stderr)?;
    }

    if let Some(stdin) = stdin {
        let stdin_file = dir.join(STDIN_FILE_NAME);
        write_contents(&stdin_file, stdin)?;
    }

    Ok(())
}

fn update(
    name: String,
    new_name: Option<String>,
    command_str: String,
    outdir: PathBuf,
    timeout: Option<u64>,
    stdin: Option<String>,
) -> Result<String> {
    let dir = outdir.join(&name);
    if !dir.exists() {
        bail!("Snapshot '{name}' does not exist. Create with 'snaprun update {name} ...'");
    }

    let old = Test::from_dir(dir.clone())?;

    let new = if let Some(new_name) = new_name {
        let new = import(new_name, command_str, outdir, timeout, stdin, false)?;
        fs::remove_dir_all(dir)?;
        new
    } else {
        import(name, command_str, outdir, timeout, stdin, true)?
    };

    let diff = SnapshotDiff::new(old, new);
    let diff = diff.render();
    Ok(diff)
}

fn import(
    name: String,
    command_str: String,
    outdir: PathBuf,
    timeout: Option<u64>,
    stdin: Option<String>,
    overwrite: bool,
) -> Result<Test> {
    let dir = outdir.join(&name);
    debug!("{}", dir.display());
    if !overwrite && dir.exists() {
        bail!("Snapshot '{name}' already exists. Modify with 'snaprun edit {name} ...'")
    }

    let parts: Vec<String> = command_str
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    let command_name = &parts[0];
    let args = if parts.len() > 1 { &parts[1..] } else { &[] };

    let Ok(exec) = which::which(&command_name) else {
        bail!("Command '{command_name}' not found");
    };
    let output = run_cmd(&exec, args, None, None)?;

    fs::create_dir_all(&dir)?;

    let test = TestBuilder::new(command_name.to_string(), dir.clone())
        .name(name.to_string())
        .stdout(output.stdout)?
        .stderr(output.stderr)?
        .stdin(stdin)?
        .timeout(timeout)
        .build()?;
    test.save()?;

    Ok(test)
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();
    match args.command() {
        args::Command::New {
            command,
            outdir,
            name,
            timeout,
            stdin,
        } => {
            let test = import(name, command, outdir, timeout, stdin, false)?;
            println!("Created snapshot '{}'", test.name);
            for file in test.list_files() {
                println!("{INDENT}{}", file.display());
            }
        }
        args::Command::Update {
            name,
            new_name,
            command,
            timeout,
            stdin,
            outdir,
        } => {
            let diff = update(
                name.clone(),
                new_name.clone(),
                command,
                outdir,
                timeout,
                stdin,
            )?;
            let name = match new_name {
                Some(name) => name,
                None => name,
            };
            println!("Updated snapshot '{}'", name);
            println!("{diff}");
        }
        args::Command::Run {
            fail_fast,
            snapshots,
        } => {
            let snapshots_dir = match snapshots {
                Some(dir) => dir,
                None => discover_snapshots()?,
            };

            let tests = discover(snapshots_dir)?;
            let results = run(&tests, fail_fast);
            display_results(&results);
        }
    }
    Ok(())
}
