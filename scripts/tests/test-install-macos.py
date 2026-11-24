#!/usr/bin/env python3
"""Exercise the installer with fake Darwin tools and disposable destinations."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory


INSTALLER = Path(__file__).resolve().parents[1] / "install-macos.sh"
MOCKS = ("uname", "stat", "cargo", "bundle.sh", "install", "xattr", "ditto", "plutil", "mv")
BUNDLE_ID = "dev.larpcc.settings"


def create_app(path, version):
    plist = path / "Contents/Info.plist"
    plist.parent.mkdir(parents=True)
    plist.write_text(BUNDLE_ID)
    (plist.parent / "version").write_text(version)


def mark(binary, root):
    marker = root / "markers" / str(binary.stat().st_ino)
    marker.write_text(hashlib.sha256(binary.read_bytes()).hexdigest())


def mock(command, args):
    root = Path(os.environ["INSTALL_TEST_ROOT"])
    race = os.environ["INSTALL_TEST_RACE"]
    home = root / "home"
    binary = home / ".local/bin/larp"
    app = home / "Applications/LarpCC.app"
    if command == "uname":
        print("Darwin")
    elif command == "stat":
        info = Path(args[-1]).lstat()
        identity = f"{info.st_dev}:{info.st_ino}"
        if args[1] == "%d:%i":
            print(identity)
        else:
            assert args[1] == "%d:%i:%z:%m:%c", args
            print(f"{identity}:{info.st_size}:{info.st_mtime_ns}:{info.st_ctime_ns}")
    elif command == "cargo":
        output = Path("target/release/larp")
        output.parent.mkdir(parents=True)
        output.write_text("new CLI")
    elif command == "bundle.sh":
        create_app(Path("apps/macos/dist/LarpCC.app"), "new app")
        if race == "prefix-symlink":
            binary.parent.parent.rename(home / "original-prefix")
            binary.parent.parent.symlink_to(home / ".codex-test")
        elif race == "applications-recreated":
            app.parent.rename(home / "original-applications")
            app.parent.mkdir()
            (app.parent / ".larp-install.lock").mkdir()
        elif race == "binary-appears":
            binary.write_text("external CLI")
        elif race == "binary-changes":
            binary.write_text("changed CLI")
            mark(binary, root)
        elif race == "app-appears":
            create_app(app, "external app")
    elif command == "install":
        shutil.copyfile(args[-2], args[-1])
    elif command == "ditto":
        shutil.copytree(args[-2], args[-1])
    elif command == "plutil":
        print(Path(args[-1]).read_text())
    elif command == "xattr":
        marker = root / "markers" / str(Path(args[-1]).stat().st_ino)
        if args[0] == "-w":
            marker.write_text(args[-2])
        elif marker.is_file():
            print(marker.read_text())
        else:
            return 1
    elif command == "mv":
        source = Path(args[-2])
        destination = Path(args[-1]) / source.name
        counter = root / "moves"
        step = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(step))
        if race == f"before-{step}":
            return 1
        if args[0] == "-nh" and os.path.lexists(destination):
            return 0
        source.rename(destination)
        if race == f"after-{step}":
            return 1
    return 0


def case(race="", old_binary=None, old_app=None, flags=()):
    with TemporaryDirectory(prefix="larp-installer-test-", dir=Path.home()) as temp:
        root = Path(temp)
        home = root / "home"
        binary = home / ".local/bin/larp"
        app = home / "Applications/LarpCC.app"
        repo = root / "repo"
        installer = repo / "scripts/install-macos.sh"
        fake_bin = root / "bin"
        for directory in (installer.parent, repo / "apps/macos/scripts", fake_bin, root / "markers",
                          binary.parent, app.parent, home / ".codex-test/bin"):
            directory.mkdir(parents=True)
        shutil.copyfile(INSTALLER, installer)
        for command in MOCKS:
            destination = repo / "apps/macos/scripts" if command == "bundle.sh" else fake_bin
            (destination / command).symlink_to(Path(__file__).resolve())
        if old_binary is not None:
            binary.write_text(old_binary)
            if old_binary == "old CLI":
                mark(binary, root)
        if old_app is not None:
            create_app(app, old_app)
        result = subprocess.run(
            ["/bin/bash", str(installer), *flags],
            env={
                "PATH": f"{fake_bin}:/usr/bin:/bin",
                "HOME": str(home),
                "LARPCC_HOME": str(root / "state"),
                "INSTALL_TEST_ROOT": str(root),
                "INSTALL_TEST_RACE": race,
            },
            capture_output=True,
            text=True,
            timeout=15,
        )
        context = (race, flags, result.returncode, result.stdout, result.stderr)
        assert (result.returncode == 0) == (not race or "--replace" in flags), context
        if "--help" in flags or "--dry-run" in flags:
            assert not (repo / "target").exists(), context
        elif race == "prefix-symlink":
            assert "Installation directory changed" in result.stderr, context
            assert not list((home / ".codex-test/bin").iterdir()), context
            assert not list(app.parent.iterdir()), context
            assert not list((home / "original-prefix/bin").iterdir()), context
        elif race == "applications-recreated":
            assert "Installation directory changed" in result.stderr, context
            assert list(app.parent.iterdir()) == [app.parent / ".larp-install.lock"], context
            assert not list(binary.parent.iterdir()), context
        else:
            if result.returncode == 0:
                expected_binary, expected_app = "new CLI", "new app"
            else:
                expected_binary = {
                    "binary-appears": "external CLI",
                    "binary-changes": "changed CLI",
                }.get(race, old_binary)
                expected_app = "external app" if race == "app-appears" else old_app
            actual_binary = binary.read_text() if binary.exists() else None
            actual_app = (app / "Contents/version").read_text() if app.exists() else None
            assert (actual_binary, actual_app) == (expected_binary, expected_app), context
            assert not list(app.parent.glob(".larp-install*")), context
            stages = list(binary.parent.glob(".larp-install*"))
            if "--replace" in flags:
                assert len(stages) == 1, context
                backup = stages[0] / "previous/larp"
                assert backup.read_text() == "external CLI" and str(backup) in result.stderr, context
            else:
                assert not stages, context


if __name__ == "__main__":
    command = Path(sys.argv[0]).name
    if command in MOCKS:
        sys.exit(mock(command, sys.argv[1:]))
    case(flags=("--help",))
    case(flags=("--dry-run",))
    case()
    case(old_binary="old CLI", old_app="old app")
    for race in ("prefix-symlink", "applications-recreated", "binary-appears"):
        case(race)
    case("binary-changes", old_binary="old CLI", old_app="old app")
    case("app-appears", old_binary="old CLI")
    case("binary-appears", flags=("--replace",))
    for timing in ("before", "after"):
        for step in range(1, 5):
            case(f"{timing}-{step}", old_binary="old CLI", old_app="old app")
        for step in range(1, 3):
            case(f"{timing}-{step}")
    print("22 mocked Darwin installer cases passed")
