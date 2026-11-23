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
    gate_with_rows(|_| Ok(std::borrow::Cow::Borrowed(admission::ROWS)))
}

pub(crate) fn gate_with_rows(
    rows: impl FnOnce(&[OsString]) -> Result<std::borrow::Cow<'static, [admission::ArtifactRow]>, Error>,
) -> Option<ExitCode> {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let name = match invocation(&argv0) {
        Ok(Some(name)) => name,
        // The image named larp enters the regular parser without reading a receipt.
        Ok(None) => match std::env::current_exe().map(|p| invocation(p.as_os_str())) {
            Ok(Ok(None)) => return None,
            _ => return Some(super::operational_error(Error::AliasInvalid)),
        },
        Err(error) => return Some(super::operational_error(error)),
    };
    Some(match dispatch(&name, &argv0, args.collect(), rows) {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => super::operational_error(error),
    })
}

fn dispatch(
    name: &ProfileName,
    argv0: &OsStr,
    args: Vec<OsString>,
    rows: impl FnOnce(&[OsString]) -> Result<std::borrow::Cow<'static, [admission::ArtifactRow]>, Error>,
) -> Result<u32, Error> {
    let executable = std::env::current_exe().map_err(|_| Error::AliasInvalid)?;
    let (receipt, digest) = Receipt::adjacent(&executable, name)?;
    receipt.running(&executable)?;
    let surface = receipt.surface();
    let registry = Registry::at(receipt.root().to_owned())?;
    let (profile, pin, _held) = registry.alias_target(receipt, digest, &executable, argv0)?;
    let registry = registry.pin_alias(pin);
    match surface {
        Surface::Cli => {
            let rows = rows(&args)?;
            admission::invoke_pinned(registry, &profile, args, &rows)
        }
        Surface::Desktop => {
            if !args.is_empty() {
                return Err(Error::ArgConflict {
                    label: "argument syntax",
                });
            }
            crate::desktop::preflight()?;
            let receipt = registry.open_desktop(&profile, false)?;
            println!("Desktop {}.", receipt.outcome);
            Ok(0)
        }
    }
}
