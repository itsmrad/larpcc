//! Local process fixture for Stage 13. Never launches SSH or accesses credentials.
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::ExitCode,
    thread,
    time::Duration,
};

const READY: &[u8] = b"LARPCC-IMPORT/1 READY\n";
const VERSION: &str = if cfg!(windows) {
    "OpenSSH_for_Windows_9.5p1, LibreSSL"
} else {
    "OpenSSH_9.5p1, LibreSSL"
};

fn file(name: &str) -> Vec<u8> {
    fs::read(PathBuf::from(name)).unwrap()
}

fn copy(name: &str, mut stream: impl Write) {
    stream.write_all(&file(name)).unwrap();
    stream.flush().unwrap();
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["-V"] {
        eprintln!("{VERSION}");
        return ExitCode::SUCCESS;
    }
    let executable = std::env::current_exe().unwrap();
    std::env::set_current_dir(executable.parent().unwrap()).unwrap();
    assert_eq!(args.last().map(String::as_str), Some("sh -s"));
    fs::write("argv.json", serde_json::to_vec(&args).unwrap()).unwrap();
    let scenario = fs::read_to_string("scenario").unwrap();
    let input = io::stdin();
    let mut input = BufReader::new(input.lock());
    let mut script = Vec::new();
    loop {
        let start = script.len();
        let bytes = input.read_until(b'\n', &mut script).unwrap();
        if bytes == 0 || script[start..].starts_with(b"exec ") {
            break;
        }
        assert!(script.len() <= 64 * 1024);
    }
    fs::write("script", &script).unwrap();
    match scenario.trim_end() {
        "timeout" => thread::sleep(Duration::from_secs(10)),
        "large-out" | "large-err" => {
            let bytes = vec![b'x'; 1024 * 1024 + 1];
            if scenario.starts_with("large-out") {
                io::stdout().write_all(&bytes).unwrap();
            } else {
                io::stderr().write_all(&bytes).unwrap();
            }
        }
        "missing" => return ExitCode::from(44),
        "unsupported" => return ExitCode::from(45),
        "refusal" => {
            eprintln!("SSH refusal sentinel");
            return ExitCode::from(255);
        }
        "banner" | "warning" => {
            copy("reply", io::stdout());
            eprintln!("{} sentinel", scenario.trim_end());
        }
        "import" | "early" => {
            if scenario.starts_with("import") {
                assert!(
                    input.buffer().is_empty(),
                    "sender sent bytes before readiness"
                );
                io::stdout().write_all(READY).unwrap();
                io::stdout().flush().unwrap();
                let mut bytes = Vec::new();
                input.read_to_end(&mut bytes).unwrap();
                fs::write("payload", bytes).unwrap();
            }
            copy("reply", io::stdout());
        }
        "reply" => copy("reply", io::stdout()),
        _ => panic!("unknown fake SSH scenario"),
    }
    let code = fs::read_to_string("exit")
        .ok()
        .and_then(|text| text.trim_end().parse().ok())
        .unwrap_or(0);
    ExitCode::from(code)
}
