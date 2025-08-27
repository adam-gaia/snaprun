use crate::types::{Check, CheckType};
use anyhow::{Context, Result, bail};
use log::debug;
use minijinja::{Environment, path_loader, value::Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[macro_export]
macro_rules! fail {
    ($($arg:tt)*) => {
        return Ok(CheckStatus::Fail {reason: format!($($arg)*)})
    };
}

#[derive(Debug)]
pub enum CheckStatus {
    Success,
    Fail { reason: String },
}

fn stream_matches(
    stream: &Vec<u8>,
    expected_match: Option<&String>,
    contains: &[String],
    stream_type: &str,
) -> CheckStatus {
    if let Some(expected_match) = expected_match {
        let actual = String::from_utf8_lossy(stream);
        if actual != *expected_match {
            return CheckStatus::Fail {
                reason: format!("{stream_type} did not match expected output"),
            };
        }

        for fragment in contains {
            if !actual.contains(fragment) {
                return CheckStatus::Fail {
                    reason: format!("{stream_type} did not contain expected fragment '{fragment}'"),
                };
            }
        }
    }

    CheckStatus::Success
}

pub fn run_command(cmd: &str, cwd: &Path, variables: &HashMap<String, String>) -> Result<Output> {
    let args = shlex::split(cmd).unwrap();
    let Some((exec, args)) = args.split_first() else {
        bail!("Unable to parse command {cmd}");
    };
    let Ok(output) = Command::new(exec)
        .args(args)
        .current_dir(cwd)
        .envs(variables)
        .output()
    else {
        bail!("Unable to run command {cmd}");
    };
    Ok(output)
}

#[derive(Debug)]
struct DiffInput<'a> {
    name: &'a str,
    content: &'a str,
}

impl<'a> DiffInput<'a> {
    pub fn new(name: &'a str, content: &'a str) -> Self {
        Self { name, content }
    }
}

fn display_str(num: Option<usize>) -> String {
    if let Some(num) = num {
        num.to_string()
    } else {
        String::from(" ")
    }
}

/// Compare 'expected' to 'actual', where 'actual' is (well probably) a modified version of 'expected'
/// Returns true if the inputs are the same, false if different.
/// If 'print' is true and there are differences, prints the diff
fn string_diff(expected: DiffInput, actual: DiffInput, print: bool) -> bool {
    // TODO: pull this function out into its own crate?
    let diff = diff::lines(expected.content, actual.content);
    let same = diff.len() == 0;

    if !same && print {
        println!("---- expected: {}", expected.name);
        println!("++++ actual: {}", actual.name);

        let indent = "  ";

        let mut line_num_a = 0;
        let mut line_num_b = 0;
        for line in diff {
            let (sep, content, line_num_a_display, line_num_b_display) = match line {
                diff::Result::Left(l) => {
                    line_num_a += 1;
                    ('-', l, Some(line_num_a), None)
                }
                diff::Result::Both(l, _) => {
                    line_num_a += 1;
                    line_num_b += 1;
                    ('|', l, Some(line_num_a), Some(line_num_b))
                }
                diff::Result::Right(r) => {
                    line_num_b += 1;
                    ('+', r, None, Some(line_num_b))
                }
            };

            let line_num_a_display = display_str(line_num_a_display);
            let line_num_b_display = display_str(line_num_b_display);

            println!("{indent}{line_num_a_display}{indent}{line_num_b_display} {sep} {content}");
        }
    }

    return same;
}

pub fn run_check(
    check: &CheckType,
    base: &Path,
    variables: &HashMap<String, String>,
    jinja_env: &Environment,
) -> Result<CheckStatus> {
    let print_diffs = true; // TODO: make configurable

    match check {
        CheckType::File {
            path,
            contains,
            template,
            contents,
        } => {
            let full = base.join(path);
            if !full.is_file() {
                fail!("Missing file {path}");
            }

            let Ok(actual_contents) = fs::read_to_string(&full) else {
                fail!("Unable to read file {}", full.display());
            };

            if let Some(expected_contents) = contents {
                let expected = DiffInput::new("Expected", expected_contents);
                let actual = DiffInput::new("Actual", &actual_contents);
                if !string_diff(expected, actual, print_diffs) {
                    fail!("File contents do not match expected contents");
                }
            };

            if let Some(template) = template {
                let full = base.join(path);
                if !full.is_file() {
                    fail!("Missing file {path}");
                }

                let Ok(actual_contents) = fs::read_to_string(&full) else {
                    fail!("Unable to read file {}", full.display());
                };

                let template = jinja_env.get_template(template)?;
                let rendered = template.render(variables)?;

                let expected = DiffInput::new("Template", &rendered);
                let actual = DiffInput::new("Actual", &actual_contents);
                if !string_diff(expected, actual, print_diffs) {
                    fail!("File contents do not match rendered template");
                }
            }

            // TODO: regex matching would be nice
            if !contains.is_empty() {
                // TODO: turn this into function to be more DRY
                for fragment in contains {
                    if !actual_contents.contains(fragment) {
                        fail!("{path}  did not contain expected fragment '{fragment}'");
                    }
                }
            }
        }

        CheckType::Directory { path, children } => {
            let full = base.join(path);
            if !full.is_dir() {
                fail!("Missing directory: {path}");
            }

            let Ok(walkdir) = fs::read_dir(&full) else {
                fail!("Unable to read directory {path}");
            };
            let actual_children: Vec<String> = walkdir
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();

            // Empty children vec doesn't signify no children expected, it means the check wasn't specified
            // More (unspecified) children may exist and thats ok
            for child in children {
                if !actual_children.contains(child) {
                    fail!("Expected child {child} of {path} does not exist");
                }
            }
        }

        CheckType::Command {
            cmd,
            code,
            expected_stdout,
            expected_stderr,
            stdout_contains,
            stderr_contains,
        } => {
            let output = match run_command(cmd, base, variables) {
                Ok(output) => output,
                Err(e) => fail!("Command did not run successfully: {e}"),
            };

            if output.status.code() != Some(*code) {
                fail!("Command {} exited with unexpected code", cmd);
            }

            let stdout = &output.stdout;
            if let CheckStatus::Fail { reason } =
                stream_matches(stdout, expected_stdout.as_ref(), &stdout_contains, "stdout")
            {
                return Ok(CheckStatus::Fail { reason });
            };

            let stderr = &output.stderr;
            if let CheckStatus::Fail { reason } =
                stream_matches(stderr, expected_stderr.as_ref(), &stderr_contains, "stderr")
            {
                return Ok(CheckStatus::Fail { reason });
            };
        }

        CheckType::Http {
            method,
            code,
            url,
            body_contains,
            expected_body,
        } => {
            todo!();
        }

        CheckType::VarSet { key, value } => {
            if let Some(value) = value {
                if let Some(actual_value) = variables.get(key) {
                    if actual_value != value {
                        fail!(
                            "Variable '{key}' did not match expected value '{value}' (was '{actual_value}')"
                        );
                    }
                } else {
                    fail!("Variable '{key}' not set");
                }
            } else {
                // Just check if key exists
                if !variables.contains_key(key) {
                    fail!("Variable '{key}' not set");
                }
            }
        }
    }

    Ok(CheckStatus::Success)
}
