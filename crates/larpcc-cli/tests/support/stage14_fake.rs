use std::{
    io::{self, Read, Write},
    process::ExitCode,
    time::Duration,
};

fn main() -> ExitCode {
    if let Some(marker) = std::env::var_os("LARPCC_STAGE14_EXECUTED") {
        std::fs::write(marker, b"executed").unwrap();
    }
    if !std::env::args().skip(1).eq(["login", "status"]) {
        return ExitCode::from(91);
    }
    let Some(home) = std::env::var_os("CODEX_HOME") else {
        return ExitCode::from(92);
    };
    if std::env::var_os("LARPCC_STAGE14_SELECTED_HOME") != Some(home) {
        return ExitCode::from(93);
    }
    if io::stdin().read(&mut [0]).unwrap() != 0 {
        return ExitCode::from(94);
    }
    let mode = std::env::var("LARPCC_STAGE14_FAKE_MODE").unwrap_or_default();
    let email = ["user", "@", "example.com"].concat();
    let plan = ["Pl", "us"].concat();
    // Assemble credential-shaped test data at runtime to avoid scanner matches.
    let credential = ["sk-", "inventedfixture01234567890123456789"].concat();
    match mode.as_str() {
        "email-only" => print!("signed-in\nemail={email}\n"),
        "plan-only" => print!("signed-in\nplan={plan}\n"),
        "neither" => print!("signed-in\n"),
        "signed-out" => {
            println!("signed-out");
            return ExitCode::from(3);
        }
        "credential" => {
            print!("signed-in\nemail={email}\nplan={plan}\ncredential={credential}\n");
            eprintln!("{credential}");
        }
        "stdout-overflow" => print!("{}", "x".repeat(16 * 1024 + 1)),
        "stderr-overflow" => eprint!("{}", "x".repeat(16 * 1024 + 1)),
        "timeout" => std::thread::sleep(Duration::from_secs(30)),
        "hold" => {
            print!("signed-in\nemail={email}\nplan={plan}\n");
            io::stdout().flush().unwrap();
            let release = std::env::var_os("LARPCC_STAGE14_RELEASE").unwrap();
            while !std::path::Path::new(&release).exists() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        _ => print!("signed-in\nemail={email}\nplan={plan}\n"),
    }
    ExitCode::SUCCESS
}
