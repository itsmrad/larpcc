//! Check desktop process activity using bounded argument-vector probes.

#[cfg(any(test, target_os = "macos"))]
use crate::registry::Error;
#[cfg(any(test, target_os = "macos"))]
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

#[cfg(any(test, target_os = "macos"))]
fn user_data_dir(bytes: &[u8]) -> Result<Option<&[u8]>, Error> {
    let (argc, bytes) = bytes.split_first_chunk::<4>().ok_or(Error::AuthBusy)?;
    let argc = i32::from_ne_bytes(*argc);
    if !(1..=4096).contains(&argc) {
        return Err(Error::AuthBusy);
    }
    let path_end = bytes.iter().position(|b| *b == 0).ok_or(Error::AuthBusy)?;
    if path_end == 0 {
        return Err(Error::AuthBusy);
    }
    let bytes = &bytes[path_end + 1..];
    let argv = bytes.iter().position(|b| *b != 0).ok_or(Error::AuthBusy)?;
    let mut bytes = &bytes[argv..];
    let mut data_dir = None;
    let mut value_next = false;
    for index in 0..argc {
        let end = bytes.iter().position(|b| *b == 0).ok_or(Error::AuthBusy)?;
        let arg = &bytes[..end];
        bytes = &bytes[end + 1..];
        if index == 0 {
            if arg.is_empty() {
                return Err(Error::AuthBusy);
            }
            continue;
        }
        let value = if value_next {
            value_next = false;
            Some(arg)
        } else if arg == b"--user-data-dir" {
            value_next = true;
            None
        } else {
            arg.strip_prefix(b"--user-data-dir=")
        };
        if let Some(value) = value
            && (!value.starts_with(b"/") || data_dir.replace(value).is_some())
        {
            return Err(Error::AuthBusy);
        }
    }
    if value_next {
        return Err(Error::AuthBusy);
    }
    Ok(data_dir)
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum App {
    ChatGpt,
    Claude,
}

#[cfg(any(test, target_os = "macos"))]
type StartTime = (u64, u64);

#[cfg(any(test, target_os = "macos"))]
struct Stamp {
    start: StartTime,
    exiting: bool,
}

#[cfg(any(test, target_os = "macos"))]
fn candidate(path: &Path) -> Option<(App, bool)> {
    for contents in path.ancestors().skip(1) {
        if contents.file_name().is_some_and(|name| name == "Contents") {
            let Some(bundle) = contents.parent().and_then(Path::file_name) else {
                continue;
            };
            let app = if bundle == "ChatGPT.app" {
                App::ChatGpt
            } else if bundle == "Claude.app" {
                App::Claude
            } else {
                continue;
            };
            let main = match app {
                App::ChatGpt => "MacOS/ChatGPT",
                App::Claude => "MacOS/Claude",
            };
            return Some((app, path.strip_prefix(contents).ok()? == Path::new(main)));
        }
    }
    None
}

#[cfg(any(test, target_os = "macos"))]
trait ProcessTable {
    fn pids(&self) -> Result<Vec<i32>, Error>;
    fn executable(&self, pid: i32) -> io::Result<Option<PathBuf>>;
    fn stamp(&self, pid: i32, previous: Option<StartTime>) -> Result<Option<Stamp>, ScanError>;
    fn validate(&self, pid: i32, app: App, main: bool) -> Result<(), Error>;
    fn args(&self, pid: i32) -> Result<Option<Vec<u8>>, Error>;
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
enum ScanError {
    Busy,
    CandidateUncertain,
    Deadline,
}

#[cfg(any(test, target_os = "macos"))]
impl From<Error> for ScanError {
    fn from(_: Error) -> Self {
        Self::Busy
    }
}

#[cfg(any(test, target_os = "macos"))]
fn retry_scan(
    mut scan: impl FnMut(&mut HashMap<i32, Option<StartTime>>) -> Result<(), ScanError>,
    wait: impl FnOnce(),
) -> Result<(), Error> {
    let mut uncertain = HashMap::new();
    let result = match scan(&mut uncertain) {
        Err(ScanError::CandidateUncertain) => {
            wait();
            scan(&mut uncertain)
        }
        Err(ScanError::Deadline) => scan(&mut uncertain),
        result => result,
    };
    result.map_err(|_| Error::AuthBusy)
}

#[cfg(any(test, target_os = "macos"))]
fn scan(
    table: &impl ProcessTable,
    expected: &HashSet<Vec<u8>>,
    uncertain: &mut HashMap<i32, Option<StartTime>>,
    check_deadline: impl Fn() -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    let previous = std::mem::take(uncertain);
    for pid in table.pids()? {
        check_deadline()?;
        let path = match (table.executable(pid), previous.get(&pid)) {
            (Ok(Some(path)), _) => path,
            (_, Some(&start)) => {
                // The earlier scan already identified a candidate at this PID.
                if table.stamp(pid, start)?.is_some() {
                    return Err(ScanError::CandidateUncertain);
                }
                continue;
            }
            (Ok(None), _) => continue,
            (Err(e), _) if e.kind() == io::ErrorKind::PermissionDenied => continue,
            (Err(e), _) if e.kind() == io::ErrorKind::NotFound => {
                scan_without_path(table, pid, expected, &check_deadline)?;
                continue;
            }
            (Err(_), _) => return Err(ScanError::Busy),
        };
        let Some(app) = candidate(&path) else {
            continue;
        };
        let mut start = None;
        match scan_candidate(
            table,
            pid,
            &path,
            app,
            expected,
            &mut start,
            &check_deadline,
        ) {
            Ok(()) => (),
            Err(ScanError::CandidateUncertain) => {
                uncertain.insert(pid, start);
            }
            Err(error) => return Err(error),
        }
    }
    check_deadline()?;
    if !uncertain.is_empty() {
        return Err(ScanError::CandidateUncertain);
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn resolved_profile_dir(dir: &[u8]) -> Result<Option<Vec<u8>>, ScanError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        // A different spelling cannot establish that the directory is unrelated.
        let resolved = Path::new(std::ffi::OsStr::from_bytes(dir))
            .canonicalize()
            .map_err(|_| ScanError::Busy)?;
        Ok(Some(resolved.as_os_str().as_bytes().to_vec()))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        // shortcut: non-unix tests compare the raw spelling; upgrade when delete scans processes there.
        Ok(None)
    }
}

#[cfg(any(test, target_os = "macos"))]
fn scan_without_path(
    table: &impl ProcessTable,
    pid: i32,
    expected: &HashSet<Vec<u8>>,
    check_deadline: impl Fn() -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    let Some(before) = table.stamp(pid, None).map_err(|_| ScanError::Busy)? else {
        return Ok(());
    };
    if before.exiting {
        return Err(ScanError::Busy);
    }
    check_deadline()?;
    let args = table.args(pid);
    let Some(after) = table
        .stamp(pid, Some(before.start))
        .map_err(|_| ScanError::Busy)?
    else {
        // The later stamp proved this PID exited or was reused.
        return Ok(());
    };
    if after.exiting {
        return Err(ScanError::Busy);
    }
    let Some(args) = args? else {
        // ESRCH or missing argv does not clear a process that is still this start time.
        return Err(ScanError::Busy);
    };
    let dir = user_data_dir(&args)?;
    if dir.is_some_and(|dir| expected.contains(dir)) {
        return Err(ScanError::Busy);
    }
    let Some(dir) = dir else {
        return Ok(());
    };
    let Some(resolved) = resolved_profile_dir(dir)? else {
        return Ok(());
    };
    let Some(again) = table
        .stamp(pid, Some(before.start))
        .map_err(|_| ScanError::Busy)?
    else {
        return Ok(());
    };
    if again.exiting || expected.contains(&resolved) {
        return Err(ScanError::Busy);
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn scan_candidate(
    table: &impl ProcessTable,
    pid: i32,
    path: &Path,
    (app, main): (App, bool),
    expected: &HashSet<Vec<u8>>,
    start: &mut Option<StartTime>,
    check_deadline: impl Fn() -> Result<(), ScanError>,
) -> Result<(), ScanError> {
    let Some(stamp) = table.stamp(pid, None)? else {
        return Ok(());
    };
    let before = stamp.start;
    *start = Some(before);
    if stamp.exiting {
        return Err(ScanError::CandidateUncertain);
    }
    let validation = table.validate(pid, app, main);
    if !same_process(table, pid, before, path)? {
        return Ok(());
    }
    validation?;
    check_deadline()?;
    let args = table.args(pid);
    if !same_process(table, pid, before, path)? {
        return Ok(());
    }
    let Ok(Some(args)) = args else {
        return Err(ScanError::CandidateUncertain);
    };
    let dir = user_data_dir(&args).map_err(|_| ScanError::CandidateUncertain)?;
    if dir.is_some_and(|dir| expected.contains(dir)) {
        return Err(ScanError::Busy);
    }
    let Some(dir) = dir else {
        return Ok(());
    };
    let Some(resolved) = resolved_profile_dir(dir)? else {
        return Ok(());
    };
    // The filesystem resolve sits between identity checks. Recheck before clearing.
    if !same_process(table, pid, before, path)? {
        return Ok(());
    }
    if expected.contains(&resolved) {
        return Err(ScanError::Busy);
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn same_process(
    table: &impl ProcessTable,
    pid: i32,
    before: StartTime,
    path: &Path,
) -> Result<bool, ScanError> {
    let executable = table.executable(pid);
    let Some(stamp) = table.stamp(pid, Some(before))? else {
        return Ok(false);
    };
    if stamp.exiting {
        return Err(ScanError::CandidateUncertain);
    }
    match executable {
        Ok(Some(current)) if current == path => Ok(true),
        Ok(Some(_)) => Err(ScanError::Busy),
        // A path lookup (including ESRCH) cannot clear a candidate with the same live stamp.
        Ok(None) | Err(_) => Err(ScanError::CandidateUncertain),
    }
}

#[cfg(test)]
#[test]
fn stage04_delete_recovery_exact_desktop_argv() {
    fn query(argc: i32, rest: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let mut packet = argc.to_ne_bytes().to_vec();
        packet.extend_from_slice(b"/Applications/ChatGPT.app/Contents/MacOS/ChatGPT\0\0ChatGPT\0");
        packet.extend_from_slice(rest);
        user_data_dir(&packet).map(|path| path.map(<[u8]>::to_vec))
    }
    for argv in [
        b"--user-data-dir=/fixture/desktop\0".as_slice(),
        b"--user-data-dir\0/fixture/desktop\0",
    ] {
        let argc = if argv.starts_with(b"--user-data-dir=") {
            2
        } else {
            3
        };
        assert_eq!(
            query(argc, argv).unwrap().as_deref(),
            Some(b"/fixture/desktop".as_slice())
        );
    }
    assert_eq!(
        query(1, b"--user-data-dir=/environment/ignored\0").unwrap(),
        None
    );
    assert_eq!(
        query(2, b"--another=--user-data-dir=/fixture/desktop\0").unwrap(),
        None
    );
    assert!(query(2, b"--user-data-dir=/truncated").is_err());
    assert!(query(2, b"--user-data-dir\0").is_err());
    assert!(query(3, b"--user-data-dir=/a\0--user-data-dir=/b\0").is_err());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
    };

    const HELPER: &str =
        "/Applications/ChatGPT.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper";
    const TARGET: &str = "/fixture/desktop";

    struct Fixture {
        path: Option<&'static str>,
        paths: RefCell<VecDeque<io::Result<Option<PathBuf>>>>,
        stamps: RefCell<VecDeque<Option<Stamp>>>,
        args: Option<Vec<u8>>,
        expected: Option<Vec<u8>>,
        argv_missing: bool,
        reads: Cell<usize>,
        listings: Cell<usize>,
        pids: &'static [i32],
    }

    impl Fixture {
        fn candidate(stamps: &[Option<StartTime>], args: Option<&[u8]>) -> Self {
            Self {
                path: Some(HELPER),
                paths: RefCell::default(),
                stamps: RefCell::new(
                    stamps
                        .iter()
                        .map(|start| {
                            start.map(|start| Stamp {
                                start,
                                exiting: false,
                            })
                        })
                        .collect(),
                ),
                args: args.map(Vec::from),
                expected: None,
                argv_missing: false,
                reads: Cell::new(0),
                listings: Cell::new(0),
                pids: &[1],
            }
        }

        fn scan(&self, deadline: impl Fn() -> Result<(), ScanError>) -> Result<(), ScanError> {
            self.scan_with_history(&mut HashMap::new(), deadline)
        }

        fn scan_with_history(
            &self,
            uncertain: &mut HashMap<i32, Option<StartTime>>,
            deadline: impl Fn() -> Result<(), ScanError>,
        ) -> Result<(), ScanError> {
            scan(
                self,
                &HashSet::from([self
                    .expected
                    .clone()
                    .unwrap_or_else(|| TARGET.as_bytes().to_vec())]),
                uncertain,
                deadline,
            )
        }
    }

    impl ProcessTable for Fixture {
        fn pids(&self) -> Result<Vec<i32>, Error> {
            self.listings.set(self.listings.get() + 1);
            Ok(self.pids.to_vec())
        }

        fn executable(&self, _pid: i32) -> io::Result<Option<PathBuf>> {
            if let Some(path) = self.paths.borrow_mut().pop_front() {
                return path;
            }
            match self.path {
                Some(path) => Ok(Some(path.into())),
                // EPERM from proc_pidpath maps to this kind on macOS.
                None => Err(io::ErrorKind::PermissionDenied.into()),
            }
        }

        fn stamp(
            &self,
            _pid: i32,
            previous: Option<StartTime>,
        ) -> Result<Option<Stamp>, ScanError> {
            Ok(self
                .stamps
                .borrow_mut()
                .pop_front()
                .expect("unexpected stamp")
                .filter(|stamp| previous.is_none_or(|before| stamp.start == before)))
        }

        fn validate(&self, _pid: i32, app: App, main: bool) -> Result<(), Error> {
            assert_eq!(self.path, Some(HELPER));
            assert_eq!((app, main), (App::ChatGpt, false));
            Ok(())
        }

        fn args(&self, _pid: i32) -> Result<Option<Vec<u8>>, Error> {
            self.reads.set(self.reads.get() + 1);
            if self.argv_missing {
                return Ok(None);
            }
            let args = self.args.as_deref().ok_or(Error::AuthBusy)?;
            let mut packet = 2i32.to_ne_bytes().to_vec();
            packet.extend_from_slice(b"/fixture/Helper\0\0Helper\0");
            packet.extend_from_slice(args);
            Ok(Some(packet))
        }
    }

    #[test]
    fn stage04_delete_skips_unrelated_and_denied_processes_before_argv() {
        for path in [Some("/Applications/Other.app/Contents/MacOS/Other"), None] {
            let table = Fixture {
                path,
                ..Fixture::candidate(&[], None)
            };
            assert_eq!(table.scan(|| Ok(())), Ok(()));
            assert_eq!(table.reads.get(), 0);
        }
    }

    #[test]
    fn stage04_delete_skips_first_lookup_esrch_despite_pid_reuse() {
        let table = Fixture {
            // The first lookup saw ESRCH before this unrelated process reused the PID.
            paths: RefCell::new(VecDeque::from([Ok(None)])),
            path: Some("/Applications/Other.app/Contents/MacOS/Other"),
            ..Fixture::candidate(&[Some((11, 0))], None)
        };
        assert_eq!(table.scan(|| Ok(())), Ok(()));
        assert_eq!(table.stamps.borrow().len(), 1);
        assert_eq!(table.reads.get(), 0);
    }

    #[test]
    fn stage04_delete_skips_vanished_or_reused_candidate() {
        let stamp = Some((10, 0));
        // Failed argv followed by either ESRCH (no stamp) or a new start time.
        for after in [None, Some((11, 0))] {
            let table = Fixture::candidate(&[stamp, stamp, after], None);
            assert_eq!(table.scan(|| Ok(())), Ok(()));
            assert_eq!(table.reads.get(), 1);
        }
    }

    #[test]
    fn stage04_delete_distinguishes_inconclusive_and_unrelated_argv() {
        let stable = [Some((10, 0)); 3];
        for args in [None, Some(b"--user-data-dir=/truncated".as_slice())] {
            let table = Fixture::candidate(&stable, args);
            assert_eq!(table.scan(|| Ok(())), Err(ScanError::CandidateUncertain));
            assert_eq!(table.reads.get(), 1);
        }
        // A real directory keeps this an unrelated success once spelling is resolved.
        #[cfg(unix)]
        {
            let other = tempfile::tempdir().unwrap();
            let unrelated = directory_arg(other.path());
            let table = Fixture::candidate(&[Some((10, 0)); 4], Some(&unrelated));
            assert_eq!(table.scan(|| Ok(())), Ok(()));
        }
        #[cfg(not(unix))]
        {
            let table =
                Fixture::candidate(&stable, Some(b"--user-data-dir=/fixture/desktop-other\0"));
            assert_eq!(table.scan(|| Ok(())), Ok(()));
        }
    }

    #[test]
    fn stage04_delete_retries_a_deadline_at_most_once() {
        for retry_expired in [false, true] {
            let table = Fixture {
                path: None,
                ..Fixture::candidate(&[], None)
            };
            let result = retry_scan(
                |uncertain| {
                    table.scan_with_history(uncertain, || {
                        if table.listings.get() == 1 || retry_expired {
                            Err(ScanError::Deadline)
                        } else {
                            Ok(())
                        }
                    })
                },
                || panic!("deadline retry should not wait"),
            );
            assert_eq!(result.is_err(), retry_expired);
            assert_eq!(table.listings.get(), 2);
        }
    }

    #[test]
    fn stage04_delete_rechecks_an_exiting_candidate_once() {
        for remains_busy in [false, true] {
            let exiting = || Stamp {
                start: (10, 0),
                exiting: true,
            };
            let table = Fixture::candidate(&[], None);
            table
                .stamps
                .borrow_mut()
                .extend([Some(exiting()), remains_busy.then(exiting)]);
            let waits = Cell::new(0);
            let result = retry_scan(
                |uncertain| table.scan_with_history(uncertain, || Ok(())),
                || waits.set(waits.get() + 1),
            );
            assert_eq!(result.is_err(), remains_busy);
            assert_eq!(table.listings.get(), 2);
            assert_eq!(waits.get(), 1);
            assert_eq!(table.reads.get(), 0);
        }
    }

    #[test]
    fn stage04_delete_refuses_esrch_during_a_live_candidate_probe() {
        for argv_missing in [false, true] {
            let mut paths = VecDeque::new();
            for _ in 0..2 {
                paths.extend([
                    Ok(Some(HELPER.into())),
                    Ok(Some(HELPER.into())),
                    // A missing executable is inconclusive if its live stamp still matches.
                    Ok(argv_missing.then(|| HELPER.into())),
                ]);
            }
            let table = Fixture {
                argv_missing,
                paths: RefCell::new(paths),
                ..Fixture::candidate(
                    &[Some((10, 0)); 6],
                    Some(b"--user-data-dir=/fixture/desktop-other\0"),
                )
            };
            let waits = Cell::new(0);
            assert!(matches!(
                retry_scan(
                    |uncertain| table.scan_with_history(uncertain, || Ok(())),
                    || waits.set(waits.get() + 1)
                ),
                Err(Error::AuthBusy)
            ));
            assert_eq!(table.listings.get(), 2);
            assert_eq!(waits.get(), 1);
            assert_eq!(table.reads.get(), 2);
        }
    }

    #[test]
    fn stage04_delete_matching_candidate_refuses_without_retry_after_exiting_candidate() {
        let table = Fixture {
            pids: &[1, 2],
            ..Fixture::candidate(
                &[Some((10, 0)); 3],
                Some(b"--user-data-dir=/fixture/desktop\0"),
            )
        };
        table.stamps.borrow_mut().push_front(Some(Stamp {
            start: (9, 0),
            exiting: true,
        }));
        assert!(matches!(
            retry_scan(
                |uncertain| table.scan_with_history(uncertain, || Ok(())),
                || panic!("matching an active profile is final")
            ),
            Err(Error::AuthBusy)
        ));
        assert_eq!(table.listings.get(), 1);
    }

    #[test]
    fn stage04_delete_rechecks_a_remembered_candidate_after_path_failure() {
        let start = Some((10, 0));
        for error in [io::ErrorKind::PermissionDenied, io::ErrorKind::NotFound] {
            for after in [start, None, Some((11, 0))] {
                let table = Fixture {
                    paths: RefCell::new(VecDeque::from([
                        Ok(Some(HELPER.into())),
                        Ok(Some(HELPER.into())),
                        Ok(Some(HELPER.into())),
                        Err(error.into()),
                    ])),
                    ..Fixture::candidate(&[start, start, start, after], None)
                };
                let result = retry_scan(
                    |uncertain| table.scan_with_history(uncertain, || Ok(())),
                    || (),
                );
                assert_eq!(result.is_err(), after == start);
                assert_eq!(table.listings.get(), 2);
                assert_eq!(table.reads.get(), 1);
                assert!(table.stamps.borrow().is_empty());
            }
        }
    }

    #[test]
    fn stage04_delete_refuses_a_profile_match_found_on_retry() {
        let table = Fixture::candidate(
            &[Some((10, 0)); 3],
            Some(b"--user-data-dir=/fixture/desktop\0"),
        );
        table.stamps.borrow_mut().push_front(Some(Stamp {
            start: (10, 0),
            exiting: true,
        }));
        assert!(matches!(
            retry_scan(
                |uncertain| table.scan_with_history(uncertain, || Ok(())),
                || ()
            ),
            Err(Error::AuthBusy)
        ));
        assert_eq!(table.listings.get(), 2);
        assert_eq!(table.reads.get(), 1);
    }

    #[test]
    fn stage04_delete_checks_argv_when_the_executable_path_is_missing() {
        let start = Some((10, 0));
        let unrelated = b"--unrelated\0".as_slice();
        let matching = b"--user-data-dir=/fixture/desktop\0".as_slice();
        for (args, after, argv_missing, expected) in [
            (Some(unrelated), start, false, Ok(())),
            (Some(matching), start, false, Err(ScanError::Busy)),
            (None, start, false, Err(ScanError::Busy)),
            (
                Some(b"--user-data-dir\0"),
                start,
                false,
                Err(ScanError::Busy),
            ),
            (Some(matching), Some((11, 0)), false, Ok(())),
            (None, None, false, Ok(())),
            (None, None, true, Ok(())),
            (None, Some((11, 0)), true, Ok(())),
            // ESRCH with the same live stamp is not proof the process is gone.
            (None, start, true, Err(ScanError::Busy)),
        ] {
            let table = Fixture {
                path: None,
                paths: RefCell::new(VecDeque::from([Err(io::ErrorKind::NotFound.into())])),
                argv_missing,
                ..Fixture::candidate(&[start, after], args)
            };
            assert_eq!(table.scan(|| Ok(())), expected);
            assert_eq!(table.reads.get(), 1);
            assert!(table.stamps.borrow().is_empty());
        }
    }

    #[cfg(unix)]
    fn path_bytes(path: &Path) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }

    #[cfg(unix)]
    fn directory_arg(path: &Path) -> Vec<u8> {
        let mut arg = b"--user-data-dir=".to_vec();
        arg.extend(path_bytes(path));
        arg.push(0);
        arg
    }

    #[test]
    fn stage04_delete_live_enoent_without_argv_stays_busy() {
        for exiting in [false, true] {
            let table = Fixture {
                path: None,
                paths: RefCell::new(VecDeque::from([Err(io::ErrorKind::NotFound.into())])),
                argv_missing: true,
                ..Fixture::candidate(&[], None)
            };
            table.stamps.borrow_mut().extend([
                Some(Stamp {
                    start: (10, 0),
                    exiting: false,
                }),
                Some(Stamp {
                    start: (10, 0),
                    exiting,
                }),
            ]);
            assert_eq!(table.scan(|| Ok(())), Err(ScanError::Busy));
            assert_eq!(table.reads.get(), 1);
            assert!(table.stamps.borrow().is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn stage04_delete_signed_candidate_resolves_directory_aliases() {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("desktop");
        std::fs::create_dir(&desktop).unwrap();
        let link = root.path().join("alias");
        std::os::unix::fs::symlink(&desktop, &link).unwrap();
        let slash = root.path().join("desktop-slash");
        std::fs::create_dir(&slash).unwrap();
        let mut slash_arg = directory_arg(&slash);
        slash_arg.pop();
        slash_arg.extend(b"/\0");
        for (arg, expected) in [
            (
                directory_arg(&link),
                path_bytes(&desktop.canonicalize().unwrap()),
            ),
            (slash_arg, path_bytes(&slash.canonicalize().unwrap())),
        ] {
            let table = Fixture {
                expected: Some(expected),
                ..Fixture::candidate(&[Some((10, 0)); 4], Some(&arg))
            };
            assert_eq!(table.scan(|| Ok(())), Err(ScanError::Busy));
            assert_eq!(table.reads.get(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn stage04_delete_rechecks_identity_after_resolving_a_signed_candidate() {
        let other = tempfile::tempdir().unwrap();
        let arg = directory_arg(other.path());
        let table = Fixture {
            paths: RefCell::new(VecDeque::from([
                Ok(Some(HELPER.into())),
                Ok(Some(HELPER.into())),
                Ok(Some(HELPER.into())),
                Ok(Some("/Applications/Other.app/Contents/MacOS/Other".into())),
            ])),
            ..Fixture::candidate(&[Some((10, 0)); 4], Some(&arg))
        };
        assert_eq!(table.scan(|| Ok(())), Err(ScanError::Busy));
        assert!(table.paths.borrow().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn stage04_delete_unresolvable_signed_directory_stays_busy() {
        let table = Fixture::candidate(
            &[Some((10, 0)); 3],
            Some(b"--user-data-dir=/fixture/desktop-missing\0"),
        );
        assert_eq!(table.scan(|| Ok(())), Err(ScanError::Busy));
        assert_eq!(table.reads.get(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn stage04_delete_enoent_resolution_rechecks_the_start_time_before_clearing() {
        let other = tempfile::tempdir().unwrap();
        let arg = directory_arg(other.path());
        for exiting in [false, true] {
            let table = Fixture {
                path: None,
                paths: RefCell::new(VecDeque::from([Err(io::ErrorKind::NotFound.into())])),
                ..Fixture::candidate(&[], Some(&arg))
            };
            table.stamps.borrow_mut().extend([
                Some(Stamp {
                    start: (10, 0),
                    exiting: false,
                }),
                Some(Stamp {
                    start: (10, 0),
                    exiting: false,
                }),
                Some(Stamp {
                    start: (10, 0),
                    exiting,
                }),
            ]);
            let expected = if exiting {
                Err(ScanError::Busy)
            } else {
                Ok(())
            };
            assert_eq!(table.scan(|| Ok(())), expected);
            assert_eq!(table.reads.get(), 1);
            assert!(table.stamps.borrow().is_empty());
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) use macos::require_quiet;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        ffi::{OsString, c_void},
        mem::MaybeUninit,
        os::unix::ffi::{OsStrExt, OsStringExt},
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };

    const PROCESS_LIMIT: usize = 4096;
    const ARG_LIMIT: usize = 64 * 1024;
    const PROC_UID_ONLY: u32 = 4; // Darwin sys/proc_info.h
    const PROC_FLAG_INEXIT: u32 = 4; // Darwin sys/proc_info.h

    pub(in crate::registry::delete) fn require_quiet(paths: &[PathBuf]) -> Result<(), Error> {
        let table = NativeProcessTable {
            uid: unsafe { libc::geteuid() },
        };
        retry_scan(
            |uncertain| {
                let deadline = Instant::now() + Duration::from_secs(2);
                let check_deadline = || {
                    if Instant::now() >= deadline {
                        Err(ScanError::Deadline)
                    } else {
                        Ok(())
                    }
                };
                let mut expected = HashSet::new();
                for path in paths {
                    check_deadline()?;
                    expected.insert(path.as_os_str().as_bytes().to_vec());
                    match path.canonicalize() {
                        Ok(resolved) => {
                            expected.insert(resolved.as_os_str().as_bytes().to_vec());
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                        Err(_) => return Err(ScanError::Busy),
                    }
                }
                check_deadline()?;
                scan(&table, &expected, uncertain, check_deadline)
            },
            || std::thread::sleep(Duration::from_millis(150)),
        )
    }

    struct NativeProcessTable {
        uid: u32,
    }

    impl ProcessTable for NativeProcessTable {
        fn pids(&self) -> Result<Vec<i32>, Error> {
            let mut pids = [0i32; PROCESS_LIMIT];
            let capacity = std::mem::size_of_val(&pids);
            // SAFETY: the kernel writes at most capacity bytes into a live pid_t array.
            let length = unsafe {
                libc::proc_listpids(
                    PROC_UID_ONLY,
                    self.uid,
                    pids.as_mut_ptr().cast(),
                    capacity as i32,
                )
            };
            if length <= 0
                || length as usize >= capacity
                || !(length as usize).is_multiple_of(size_of::<i32>())
            {
                return Err(Error::AuthBusy);
            }
            Ok(pids
                .into_iter()
                .take(length as usize / size_of::<i32>())
                .filter(|&pid| pid > 0)
                .collect())
        }

        fn executable(&self, pid: i32) -> io::Result<Option<PathBuf>> {
            let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            // SAFETY: bounded writable path buffer.
            let len =
                unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
            if len <= 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH)
                    || process_info(pid).is_ok_and(|info| zombie(pid, &info))
                    || exited(pid)
                {
                    return Ok(None);
                }
                return Err(error);
            }
            if len as usize >= path.len() || path[0] != b'/' {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let end = path
                .iter()
                .position(|&byte| byte == 0)
                .ok_or(io::ErrorKind::InvalidData)?;
            Ok(Some(OsString::from_vec(path[..end].to_vec()).into()))
        }

        fn stamp(&self, pid: i32, previous: Option<StartTime>) -> Result<Option<Stamp>, ScanError> {
            let info = match process_info(pid) {
                Ok(info) => info,
                Err(_) if exited(pid) => return Ok(None),
                Err(_) => return Err(ScanError::CandidateUncertain),
            };
            let start = (info.pbi_start_tvsec, info.pbi_start_tvusec);
            if info.pbi_pid == pid as u32 && previous.is_some_and(|previous| previous != start)
                || zombie(pid, &info)
            {
                return Ok(None);
            }
            if info.pbi_pid != pid as u32
                || info.pbi_uid != self.uid
                || info.pbi_ruid != self.uid
                || info.pbi_svuid != self.uid
                || info.pbi_start_tvsec == 0
                || info.pbi_start_tvusec >= 1_000_000
            {
                return Err(ScanError::Busy);
            }
            Ok(Some(Stamp {
                start,
                exiting: info.pbi_flags & PROC_FLAG_INEXIT != 0,
            }))
        }

        fn validate(&self, pid: i32, app: App, main: bool) -> Result<(), Error> {
            let (identifier, team) = match app {
                App::ChatGpt => ("com.openai.codex", "2DC432GLL2"),
                App::Claude => ("com.anthropic.claudefordesktop", "Q6L2SF6YDW"),
            };
            let mut requirement = String::new();
            if main {
                requirement = format!("identifier \"{identifier}\" and ");
            }
            // Nested helpers have their own identifiers but must carry the bundle vendor's signature.
            requirement.push_str("anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = ");
            requirement.push_str(&format!("\"{team}\""));
            validate(pid, requirement.as_bytes())
        }

        fn args(&self, pid: i32) -> Result<Option<Vec<u8>>, Error> {
            let mut args = vec![0u8; ARG_LIMIT];
            let mut length = args.len();
            let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
            // SAFETY: output storage and length are bounded. Only argc argv entries are parsed.
            if unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    args.as_mut_ptr().cast(),
                    &mut length,
                    null_mut(),
                    0,
                )
            } != 0
            {
                if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                    return Ok(None);
                }
                return Err(Error::AuthBusy);
            }
            if length >= args.len() {
                return Err(Error::AuthBusy);
            }
            args.truncate(length);
            Ok(Some(args))
        }
    }

    fn exited(pid: i32) -> bool {
        // SAFETY: signal 0 only probes liveness.
        (unsafe { libc::kill(pid, 0) }) != 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn zombie(pid: i32, info: &libc::proc_bsdinfo) -> bool {
        info.pbi_pid == pid as u32 && info.pbi_status == 5
    }

    fn process_info(pid: i32) -> io::Result<libc::proc_bsdinfo> {
        let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = size_of::<libc::proc_bsdinfo>() as i32;
        // SAFETY: proc_pidinfo initializes the full output only on an exact-size response.
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        } != size
        {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { info.assume_init() })
    }

    // TODO: share the process signature predicate with desktop dispatch after #24.
    type Ref = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
        fn CFDictionaryCreate(
            allocator: Ref,
            keys: *const Ref,
            values: *const Ref,
            count: isize,
            key_callbacks: Ref,
            value_callbacks: Ref,
        ) -> Ref;
        fn CFStringCreateWithBytes(
            allocator: Ref,
            bytes: *const u8,
            len: isize,
            encoding: u32,
            external: u8,
        ) -> Ref;
        fn CFRelease(value: Ref);
    }
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecGuestAttributePid: Ref;
        fn SecCodeCopyGuestWithAttributes(
            host: Ref,
            attributes: Ref,
            flags: u32,
            code: *mut Ref,
        ) -> i32;
        fn SecRequirementCreateWithString(text: Ref, flags: u32, requirement: *mut Ref) -> i32;
        fn SecCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
    }
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self, Error> {
            if value.is_null() {
                Err(Error::AuthBusy)
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }

    fn validate(pid: i32, requirement: &[u8]) -> Result<(), Error> {
        // SAFETY: owned Create outputs are kept alive for borrowed dictionary values.
        unsafe {
            let number = Owned::new(CFNumberCreate(null(), 3, (&pid as *const i32).cast()))?;
            let key = kSecGuestAttributePid;
            let attrs = Owned::new(CFDictionaryCreate(
                null(),
                &key,
                &number.0,
                1,
                null(),
                null(),
            ))?;
            let mut code = null();
            if SecCodeCopyGuestWithAttributes(null(), attrs.0, 0, &mut code) != 0 {
                return Err(Error::AuthBusy);
            }
            let code = Owned::new(code)?;
            let text = Owned::new(CFStringCreateWithBytes(
                null(),
                requirement.as_ptr(),
                requirement.len() as isize,
                0x08000100,
                0,
            ))?;
            let mut requirement = null();
            if SecRequirementCreateWithString(text.0, 0, &mut requirement) != 0 {
                return Err(Error::AuthBusy);
            }
            let requirement = Owned::new(requirement)?;
            if SecCodeCheckValidity(code.0, 0, requirement.0) != 0 {
                return Err(Error::AuthBusy);
            }
        }
        Ok(())
    }
}
