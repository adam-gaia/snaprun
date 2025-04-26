use colored::{Color, Colorize};
use std::{fmt::Display, str::FromStr};

fn display_str(num: Option<usize>) -> String {
    if let Some(num) = num {
        num.to_string()
    } else {
        String::from(" ")
    }
}

#[derive(Debug)]
pub enum Diff {
    Same,
    Diff(String),
}

impl Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Same => write!(f, ""),
            Self::Diff(diff) => write!(f, "{diff}"),
        }
    }
}

#[derive(Debug)]
enum Side {
    Left,
    Right,
}

impl Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left => write!(f, "left"),
            Self::Right => write!(f, "right"),
        }
    }
}

fn header(side: Side, name: Option<&String>, marker: char, marker_len: usize) -> String {
    let marker_bar = marker.to_string().repeat(marker_len);

    let name = match name {
        Some(name) => format!("{side}: {name}"),
        None => format!("{side}"),
    };

    format!("{marker_bar} {name}")
}

enum ColorSide {
    Left,
    Right,
    Both,
}

/// Compare 'expected' to 'actual', where 'actual' is (well probably) a modified version of 'expected'
/// Returns true if the inputs are the same, false if different.
/// If 'print' is true and there are differences, prints the diff
pub fn line_diff(left: &str, right: &str, settings: &DiffSettings, print: bool) -> bool {
    let mut same = true;
    let diff = diff::lines(left, right);

    let left_color = if let Some(color) = settings.left_color {
        color
    } else {
        DEFAULT_LEFT_COLOR
    };

    let right_color = if let Some(color) = settings.right_color {
        color
    } else {
        DEFAULT_RIGHT_COLOR
    };

    let indent = " ".repeat(settings.indent_spaces);

    if print {
        // TODO: force color and no color should be mutually exclusive
        if settings.force_color {
            colored::control::set_override(true);
        }
        if settings.no_color {
            colored::control::set_override(false);
        }

        let left_header = header(
            Side::Left,
            settings.left_name.as_ref(),
            settings.left_marker,
            settings.marker_count,
        )
        .color(left_color);
        let right_header = header(
            Side::Right,
            settings.right_name.as_ref(),
            settings.right_marker,
            settings.marker_count,
        )
        .color(right_color);
        println!("{left_header}");
        println!("{right_header}");
    }

    let mut line_num_a = 0;
    let mut line_num_b = 0;
    for line in diff {
        let (sep, content, line_num_a_display, line_num_b_display, color) = match line {
            diff::Result::Left(l) => {
                same = false;
                line_num_a += 1;
                ('-', l, Some(line_num_a), None, ColorSide::Left)
            }
            diff::Result::Both(l, _) => {
                line_num_a += 1;
                line_num_b += 1;
                ('|', l, Some(line_num_a), Some(line_num_b), ColorSide::Both)
            }
            diff::Result::Right(r) => {
                same = false;
                line_num_b += 1;
                ('+', r, None, Some(line_num_b), ColorSide::Right)
            }
        };

        if print {
            let line_num_a_display = display_str(line_num_a_display);
            let line_num_b_display = display_str(line_num_b_display);

            let line =
                format!("{indent}{line_num_a_display}{indent}{line_num_b_display} {sep} {content}");
            let line = match color {
                ColorSide::Left => line.color(left_color),
                ColorSide::Right => line.color(right_color),
                ColorSide::Both => line.dimmed(),
            };
            println!("{line}");
        }
    }

    same
}

const DEFAULT_LEFT_MARKER: char = '-';
const DEFAULT_RIGHT_MARKER: char = '+';
const DEFAULT_MARKER_COUNT: usize = 4;
const DEFAULT_INDENT_SPACES: usize = 2;
const DEFAULT_LEFT_COLOR: Color = Color::Green;
const DEFAULT_RIGHT_COLOR: Color = Color::Red;
use anyhow::Result;

fn parse_color(s: &str) -> Result<Color> {
    todo!();
}

// TODO: settings for no line numbers, plain style, no header, etc
// TODO: tests

#[derive(Debug, Clone, clap::Parser)]
pub struct DiffSettings {
    #[clap(long)]
    left_name: Option<String>,

    #[clap(long)]
    right_name: Option<String>,

    #[clap(long, default_value_t = DEFAULT_LEFT_MARKER)]
    left_marker: char,

    #[clap(long, default_value_t = DEFAULT_RIGHT_MARKER)]
    right_marker: char,

    #[clap(long, default_value_t = DEFAULT_MARKER_COUNT)]
    marker_count: usize,

    #[clap(long, default_value_t = DEFAULT_INDENT_SPACES)]
    indent_spaces: usize,

    #[clap(short, long)]
    force_color: bool,

    #[clap(long, value_parser = parse_color)]
    left_color: Option<Color>,

    #[clap(long, value_parser = parse_color)]
    right_color: Option<Color>,

    #[clap(long)]
    no_color: bool,
}

impl DiffSettings {
    pub fn new() -> Self {
        Self::default()
    }

    // TODO full builder stuff
    pub fn names(mut self, left: String, right: String) -> Self {
        self.left_name = Some(left);
        self.right_name = Some(right);
        self
    }
}

impl Default for DiffSettings {
    fn default() -> Self {
        Self {
            left_name: None,
            right_name: None,
            left_marker: DEFAULT_LEFT_MARKER,
            right_marker: DEFAULT_RIGHT_MARKER,
            marker_count: DEFAULT_MARKER_COUNT,
            indent_spaces: DEFAULT_INDENT_SPACES,
            force_color: false,
            left_color: Some(DEFAULT_LEFT_COLOR),
            right_color: Some(DEFAULT_RIGHT_COLOR),
            no_color: false,
        }
    }
}
