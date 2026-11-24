use serde_json::Value;
use std::{fmt::Write, io::IsTerminal};

struct Terminal {
    width: Option<u16>,
    unicode: bool,
    color: bool,
}

impl Terminal {
    fn stdout() -> Self {
        let (width, ansi) = if std::io::stdout().is_terminal() {
            terminal_width()
        } else {
            (None, false)
        };
        let term = std::env::var("TERM").ok();
        let known = width.is_some()
            && term
                .as_deref()
                .is_some_and(|term| !term.is_empty() && term != "dumb");
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
            .is_some_and(|locale| {
                let locale = locale.to_ascii_lowercase();
                locale.contains("utf-8") || locale.contains("utf8")
            });
        Self {
            width,
            unicode: known && utf8,
            color: known && ansi && std::env::var_os("NO_COLOR").is_none(),
        }
    }
}

#[cfg(unix)]
fn terminal_width() -> (Option<u16>, bool) {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // TIOCGWINSZ initializes the terminal size on success.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
        return (None, false);
    }
    let columns = unsafe { size.assume_init().ws_col };
    ((columns != 0).then_some(columns), true)
}

#[cfg(windows)]
fn terminal_width() -> (Option<u16>, bool) {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
        GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    let mut size = std::mem::MaybeUninit::<CONSOLE_SCREEN_BUFFER_INFO>::zeroed();
    // Console APIs fill their output buffers only when the handle is a console.
    let stdout = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if unsafe { GetConsoleScreenBufferInfo(stdout, size.as_mut_ptr()) } == 0 {
        return (None, false);
    }
    let mut mode = 0;
    let ansi = unsafe { GetConsoleMode(stdout, &mut mode) } != 0
        && mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0;
    let rect = unsafe { size.assume_init().srWindow };
    let width = (i32::from(rect.Right) - i32::from(rect.Left) + 1).max(0) as u16;
    ((width != 0).then_some(width), ansi)
}

fn relative_time(seconds: i64) -> String {
    let minutes = (seconds.max(0) + 59) / 60;
    if minutes >= 1440 {
        format!("{}d {}h", minutes / 1440, (minutes / 60) % 24)
    } else if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

pub(crate) fn status_graph(usage: Option<&Value>, read_clock: i64) -> String {
    render(usage, read_clock, &Terminal::stdout())
}

fn render(usage: Option<&Value>, read_clock: i64, terminal: &Terminal) -> String {
    let windows = usage.map_or(&Value::Null, |usage| &usage["windows"]);
    let windows = [
        ("Session", &windows["five_hour"]),
        ("Weekly", &windows["weekly"]),
    ];
    let usable = |window: &Value| {
        matches!(window["reason"].as_str(), None | Some("reset_time_unknown"))
            && window["used_percent"].is_number()
    };
    if windows
        .iter()
        .all(|(_, window)| !usable(window) && window["reason"] != "reset_since_capture")
    {
        return "usage: not available\n".into();
    }
    let mut output = String::new();
    for (label, window) in windows {
        if window["reason"] == "reset_since_capture" {
            let _ = writeln!(output, "{label:<7} reset since last use\n        Reset due");
        } else if usable(window) {
            let used =
                (window["used_percent"].as_f64().unwrap_or_default() * 10.0).round() as usize;
            let remaining = 1000_usize.saturating_sub(used);
            let _ = write!(output, "{label:<7} ");
            if terminal.width.is_none_or(|width| width >= 48) {
                let (filled, empty) = if terminal.unicode {
                    ("█", "░")
                } else {
                    ("#", "-")
                };
                let cells = remaining * 20 / 1000;
                let bar = filled.repeat(cells);
                let gap = empty.repeat(20 - cells);
                if terminal.color && cells != 0 {
                    let _ = write!(output, "[\x1b[32m{bar}\x1b[0m{gap}] ");
                } else {
                    let _ = write!(output, "[{bar}{gap}] ");
                }
            }
            let _ = writeln!(output, "{}% left", remaining / 10);
            if let Some(reset) = window["resets_at"].as_str().and_then(super::clock::parse) {
                let _ = writeln!(
                    output,
                    "        Resets in {}",
                    relative_time(reset - read_clock)
                );
            } else {
                let _ = writeln!(output, "        Reset time unavailable");
            }
        } else {
            let _ = writeln!(output, "{label}: not available");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stage11_status_usage_graph() {
        let clock = super::super::clock::parse("2026-11-21T12:00:00Z").unwrap();
        let pipe = Terminal {
            width: None,
            unicode: false,
            color: false,
        };
        let narrow = Terminal {
            width: Some(40),
            unicode: true,
            color: true,
        };
        let mut usage = json!({"windows":{
            "five_hour":{"reason":"reset_since_capture","used_percent":null},
            "weekly":{"reason":null,"used_percent":24.1,"resets_at":"2026-11-21T13:04:01Z"}
        }});
        assert_eq!(
            render(Some(&usage), clock, &pipe),
            "Session reset since last use\n        Reset due\nWeekly  [###############-----] 75% left\n        Resets in 1h 5m\n"
        );
        assert!(!render(Some(&usage), clock, &narrow).contains('['));
        usage["windows"]["weekly"]["reason"] = json!("not_reported");
        assert!(render(Some(&usage), clock, &pipe).ends_with("Weekly: not available\n"));
        usage["windows"]["weekly"]["reason"] = json!("reset_since_capture");
        assert_eq!(
            render(Some(&usage), clock, &pipe)
                .matches("Reset due")
                .count(),
            2
        );
        assert_eq!(render(None, clock, &pipe), "usage: not available\n");
    }
}
