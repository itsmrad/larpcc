//! argv[0] dispatch for root-bound alias launchers. Runs before clap, the early
//! `--json`/`--expected-revision` scans and every other gate, so alias arguments are
//! never parsed as `cc` grammar and alias mode has no JSON envelope.
use crate::{
    aliases::{Receipt, invocation},
    launch::admission,
    registry::{Error, ProfileName, Registry, Surface},
};
use std::{
    ffi::{OsStr, OsString},
    process::ExitCode,
};

/// `None` only for the standard `larp` executable invoked as `larp`.
#[allow(
    dead_code,
    reason = "the test-only synthetic driver shares this module but has its own entrypoint"
)]
pub fn gate() -> Option<ExitCode> {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let name = match invocation(&argv0) {
        Ok(Some(name)) => name,
        // A launcher started as `larp` (argv[0] spoofed or renamed copy) must not escape
        // into the `cc` grammar: the running image must itself be named larp.
        Ok(None) => match std::env::current_exe().map(|p| invocation(p.as_os_str())) {
            Ok(Ok(None)) => return None,
            _ => return Some(super::operational_error(Error::AliasInvalid)),
        },
        Err(error) => return Some(super::operational_error(error)),
    };
    Some(match dispatch(&name, &argv0, args.collect()) {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => super::operational_error(error),
    })
}

fn dispatch(name: &ProfileName, argv0: &OsStr, args: Vec<OsString>) -> Result<u32, Error> {
    let executable = std::env::current_exe().map_err(|_| Error::AliasInvalid)?;
    let (receipt, digest) = Receipt::adjacent(&executable, name)?;
    let registry = Registry::at(receipt.root().to_owned())?;
    // Handles stay open through dispatch; admission revalidates the pinned revision.
    let (profile, revision, _held) =
        registry.alias_target(&receipt, &digest, &executable, argv0)?;
    match receipt.surface() {
        Surface::Cli => {
            admission::invoke_pinned(registry, revision, &profile, args, admission::ROWS)
        }
        Surface::Desktop => {
            if !args.is_empty() {
                return Err(Error::ArgConflict {
                    label: "argument syntax",
                });
            }
            crate::desktop::preflight()?;
            let receipt = registry
                .expecting(Some(revision))
                .open_desktop(&profile, false)?;
            println!("Desktop {}.", receipt.outcome);
            Ok(0)
        }
    }
}
