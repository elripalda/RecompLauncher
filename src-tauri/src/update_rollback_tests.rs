//! Run the real startup check and update helpers against disposable stand-in apps.

use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const CHILD: &str = "RL_TEST_UPDATE_STARTUP";
const MODE: &str = "RL_TEST_UPDATE_STARTUP_MODE";
const NEWER: &str = r#"{"to":"999.0.0","from":"998.0.0"}"#;

struct Fixture {
    dir: PathBuf,
    pending: PathBuf,
    original: Value,
    mac: bool,
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.dir); }
}

fn script(path: &Path, text: &str) {
    fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

impl Fixture {
    fn run(mac: bool, mode: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("rl-update-rollback-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap();
        let pending = dir.join("update-pending.json");
        let original = json!({"to": env!("CARGO_PKG_VERSION"), "from": "0.0.0", "at": crate::now(), "installer": "", "url": "https://example.invalid/update", "sha256": "a".repeat(64)});
        fs::write(&pending, original.to_string()).unwrap();
        let fixture = Self { dir, pending, original, mac };
        let dir = &fixture.dir;
        let child = "exec \"$RL_TEST_UPDATE_BINARY\" --exact update_rollback_tests::startup_then_crash_keeps_the_failure_record";
        let mut gone = Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        let mut command = Command::new("/bin/sh");
        if mac {
            let app = dir.join("RecompLauncher.app");
            let old = dir.join(".RecompLauncher-old.app");
            fs::create_dir(&app).unwrap();
            fs::create_dir(&old).unwrap();
            fs::write(app.join("which"), "new").unwrap();
            fs::write(old.join("which"), "old").unwrap();
            let program = dir.join("new-program");
            script(&program, child);
            command.args(["-c", include_str!("mac_update.sh"), "sh"]).arg(gone.id().to_string());
            command.arg(app).arg(old).arg(dir.join("failed.app")).arg(program).arg("/usr/bin/true").arg(&fixture.pending);
        } else {
            let app = dir.join("Launcher.AppImage");
            let work = dir.join("work");
            fs::create_dir(&work).unwrap();
            script(&app, "# old\nexit 0");
            script(&work.join("previous.AppImage"), "# old\nexit 0");
            script(&work.join("next.AppImage"), child);
            command.args(["-c", include_str!("appimage_update.sh"), "sh"]).arg(gone.id().to_string());
            command.arg(app).arg(work).arg(dir.join("launcher-trail.log")).arg("1").arg(&fixture.pending).arg("--desktop");
        }
        command.env(CHILD, dir).env(MODE, mode).env("RL_TEST_UPDATE_BINARY", std::env::current_exe().unwrap());
        let status = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
        assert_eq!(status.success(), mode != "missing-backup");
        fixture
    }

    fn old_is_restored(&self) -> bool {
        if self.mac {
            return fs::read_to_string(self.dir.join("RecompLauncher.app/which")).unwrap() == "old";
        }
        fs::read_to_string(self.dir.join("Launcher.AppImage")).unwrap().contains("# old")
    }
}

#[test]
fn startup_then_crash_keeps_the_failure_record() {
    // Run startup in a separate process, as an actual new version does, then fail during the
    // helper's watch window. No launcher UI, installed app or personal data is used.
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = PathBuf::from(dir);
        crate::update_check_last(&dir);
        let mode = std::env::var(MODE).unwrap();
        match mode.as_str() {
            "newer" => fs::write(dir.join("update-pending.json"), NEWER).unwrap(),
            "blocked" => std::os::unix::fs::symlink(dir.join("missing/record"), dir.join("update-pending.json")).unwrap(),
            "missing-backup" => {
                let old = dir.join(".RecompLauncher-old.app");
                if old.is_dir() { fs::remove_dir_all(old).unwrap(); }
                else { fs::remove_file(dir.join("work/previous.AppImage")).unwrap(); }
            }
            "success" => std::process::exit(0),
            _ => {}
        }
        std::process::exit(3);
    }
    for mac in [true, false] {
        let fixture = Fixture::run(mac, "crash");
        assert!(fixture.old_is_restored());
        let restored: Value = serde_json::from_slice(&fs::read(&fixture.pending).expect("rollback must keep the failure record")).unwrap();
        assert_eq!(restored, fixture.original);
    }
}

#[test]
fn rollback_does_not_replace_a_newer_pending_record() {
    for mac in [true, false] {
        let fixture = Fixture::run(mac, "newer");
        assert!(fixture.old_is_restored());
        assert_eq!(fs::read_to_string(&fixture.pending).unwrap(), NEWER);
    }
}

#[test]
fn a_successful_start_does_not_restore_the_pending_record() {
    for mac in [true, false] {
        let fixture = Fixture::run(mac, "success");
        assert!(!fixture.old_is_restored());
        assert!(!fixture.pending.exists());
    }
}

#[test]
fn a_failed_record_write_is_logged_without_preventing_rollback() {
    for mac in [true, false] {
        let fixture = Fixture::run(mac, "blocked");
        assert!(fixture.old_is_restored());
        assert!(fixture.pending.is_symlink());
        let trail = fs::read_to_string(fixture.dir.join("launcher-trail.log")).unwrap();
        assert!(trail.contains("Couldn't restore the update failure record"), "{trail}");
    }
}

#[test]
fn a_failed_rollback_does_not_restore_the_pending_record() {
    for mac in [true, false] {
        let fixture = Fixture::run(mac, "missing-backup");
        assert!(!fixture.old_is_restored());
        assert!(!fixture.pending.exists());
    }
}
