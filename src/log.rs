use std::sync::Mutex;
use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;

use ratatui::text::{Line, Span};
use ratatui::style::{Style, Color};

const LOG_TAIL: usize = 16;

#[derive(PartialEq, PartialOrd, Clone, Copy)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Critical = 5,
}

static LOG: Mutex<Vec<(String, LogLevel)>> = Mutex::new(vec![]);

fn to_span<'a>(level: LogLevel) -> Span<'a> {
    let (text, color) = match level {
        LogLevel::Trace => ("TRC", Color::Blue),
        LogLevel::Debug => ("DBG", Color::Cyan),
        LogLevel::Info => ("INF", Color::Green),
        LogLevel::Warn => ("WRN", Color::Yellow),
        LogLevel::Error => ("ERR", Color::Red),
        LogLevel::Critical => ("CRT", Color::Magenta)
    };
    Span::styled(text, Style::default().fg(color))
}

pub fn lines<'a>(n: usize, level: LogLevel) -> Vec<Line<'a>> {
    let log = LOG.lock().unwrap();
    let mut out = Vec::new();
    let mut index = log.len() - 1;
    while index > 0 || out.len() == n {
        let (line, line_level) = log[index].clone();
        if line_level < level {
            continue
        }
        out.push(Line::from(vec![
            Span::from("["),
            to_span(level),
            Span::from(format!("] {}", line))
        ]));
        index -= 1;
    }
    out
}

pub fn log(line: &str, level: LogLevel) {
    let time = Local::now().format("%H:%M:%S%.3f").to_string();
    let line = format!("[{}] {}", time, line);
    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .open("log.txt")
        .unwrap();
    writeln!(file, "{}", line).unwrap();
    let mut log = LOG.lock().unwrap();
    log.push((line, level));
    if log.len() > LOG_TAIL {
        *log = log.iter().cloned().skip(1).take(LOG_TAIL).collect();
    }
}

// TODO who macroes the macros

macro_rules! warning{
    ($str:ident) => {{
        crate::log::log(&$str, crate::log::LogLevel::Warn);
    }};

    ($fmt_str:literal) => {{
        crate::log::log($fmt_str, crate::log::LogLevel::Warn);
    }};

    ($fmt_str:literal, $($args:expr),*) => {{
        crate::log::log(&format!($fmt_str, $($args),*), crate::log::LogLevel::Warn);
    }};
}

macro_rules! error{
    ($str:ident) => {{
        crate::log::log(&$str, crate::log::LogLevel::Error);
    }};

    ($fmt_str:literal) => {{
        crate::log::log($fmt_str, crate::log::LogLevel::Error);
    }};

    ($fmt_str:literal, $($args:expr),*) => {{
        crate::log::log(&format!($fmt_str, $($args),*), crate::log::LogLevel::Error);
    }};
}

macro_rules! info{
    ($str:ident) => {{
        crate::log::log(&$str, crate::log::LogLevel::Info);
    }};

    ($fmt_str:literal) => {{
        crate::log::log($fmt_str, crate::log::LogLevel::Info);
    }};

    ($fmt_str:literal, $($args:expr),*) => {{
        crate::log::log(&format!($fmt_str, $($args),*), crate::log::LogLevel::Info);
    }};
}

macro_rules! debug{
    ($str:ident) => {{
        crate::log::log(&$str, crate::log::LogLevel::Debug);
    }};

    ($fmt_str:literal) => {{
        crate::log::log($fmt_str, crate::log::LogLevel::Debug);
    }};

    ($fmt_str:literal, $($args:expr),*) => {{
        crate::log::log(&format!($fmt_str, $($args),*), crate::log::LogLevel::Debug);
    }};
}

macro_rules! trace{
    ($str:ident) => {{
        crate::log::log(&$str, crate::log::LogLevel::Trace);
    }};

    ($fmt_str:literal) => {{
        crate::log::log($fmt_str, crate::log::LogLevel::Trace);
    }};

    ($fmt_str:literal, $($args:expr),*) => {{
        crate::log::log(&format!($fmt_str, $($args),*), crate::log::LogLevel::Trace);
    }};
}

pub(crate) use {trace, debug, info, warning, error};
