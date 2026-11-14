#![cfg(windows)]
mod common;
use common::*;
use std::{
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{FromRawHandle, OwnedHandle},
    },
    path::Path,
    process::{Child, Stdio},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};
fn delete_probe(path: &Path) -> io::Result<()> {
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: harmless DELETE-access query against known fixture directory; no disposition/rename.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful open transfers ownership; close without mutating the object.
    drop(unsafe { OwnedHandle::from_raw_handle(raw) });
    Ok(())
}
struct Cleanup(Option<Child>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn w07_cli_mutation_retains_actual_root_and_profiles_pins_until_completion() {
    let f = Fixture::new();
    f.profile(&["add", "work", "--provider", "claude"], 0);
    let profiles = f.root.join("profiles");
    delete_probe(&f.root).unwrap();
    delete_probe(&profiles).unwrap();
    let lock = File::options()
        .read(true)
        .write(true)
        .open(f.root.join("registry.lock"))
        .unwrap();
    lock.lock().unwrap();
    let child = f
        .command(&["cc", "profile", "rename", "work", "renamed"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut cleanup = Cleanup(Some(child));
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match delete_probe(&f.root) {
            Err(e) if e.raw_os_error() == Some(32) => break,
            Ok(()) => (),
            Err(e) => panic!("unexpected root pin probe error: {e}"),
        }
        assert!(
            cleanup.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "mutation exited before lock/pin readiness"
        );
        assert!(
            Instant::now() < end,
            "mutation did not retain its actual root pin"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        delete_probe(&profiles).unwrap_err().raw_os_error(),
        Some(32),
        "profiles pin must remain held while mutation waits"
    );
    for (path, moved) in [
        (&f.root, f.temp.path().join("moved-root")),
        (&profiles, f.root.join("moved-profiles")),
    ] {
        assert_eq!(
            std::fs::rename(path, moved).unwrap_err().raw_os_error(),
            Some(32)
        );
    }
    lock.unlock().unwrap();
    drop(lock);
    let out = wait_output(cleanup.0.take().unwrap()).unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Renamed profile work to renamed"));
    delete_probe(&f.root).unwrap();
    delete_probe(&profiles).unwrap();
    let moved = f.root.join("moved-profiles");
    std::fs::rename(&profiles, &moved).unwrap();
    std::fs::rename(moved, profiles).unwrap();
    let moved = f.temp.path().join("moved-root");
    std::fs::rename(&f.root, &moved).unwrap();
    std::fs::rename(moved, &f.root).unwrap();
    f.profile(&["show", "renamed"], 0);
}
