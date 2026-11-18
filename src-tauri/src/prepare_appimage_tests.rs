//! Executes a real Type-2 AppImage through Easy Setup, with no game files or desktop needed.

use super::*;
use backhand::compression::Compressor;
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn builder_image(runtime: &Path, path: &Path) {
    let script = br#"#!/bin/sh
set -eu
case "$APPDIR" in
    "$XDG_CACHE_HOME"/appimage-run/*) ;;
    *) echo 'The builder must run from the disk extraction directory' >&2; exit 41 ;;
esac
read -r value < "$1"
printf '%s\n' "$((value * value))" > "$2"
"#;
    let mut filesystem = FilesystemWriter::default();
    filesystem.set_root_mode(0o755);
    filesystem.set_compressor(FilesystemCompressor::new(Compressor::Gzip, None).unwrap());
    let header = NodeHeader { permissions: 0o755, ..Default::default() };
    filesystem.push_file(Cursor::new(script.to_vec()), "/AppRun", header).unwrap();
    let mut payload = Cursor::new(Vec::new());
    filesystem.write(&mut payload).unwrap();
    let mut image = std::fs::read(runtime).unwrap();
    image.extend(payload.into_inner());
    std::fs::write(path, image).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[ignore = "Manual integration test; provide RL_TEST_APPIMAGE_RUNTIME as described in docs/BUILDING.md"]
fn appimage_prepare_runs_from_disk_without_fuse() {
    if let Some(directory) = std::env::var_os("RL_APPIMAGE_TEST_DIR") {
        let directory = PathBuf::from(directory);
        let input = directory.join("input value.txt");
        std::fs::write(&input, "12\n").unwrap();
        let prepare = profile::Prepare {
            program: vec!["Test Builder.APPIMAGE".into()],
            args: vec!["{game}".into(), "{install}/prepared result.txt".into()],
            label: String::new(),
            timeout: 30,
            options: vec![],
            version: String::new(),
            done: vec![],
        };
        run_prepare(&prepare, &directory, &input, &[], &|_, _| {}, &|| false).unwrap();
        let result = std::fs::read_to_string(directory.join("prepared result.txt")).unwrap();
        assert_eq!(result, "144\n");
        return;
    }

    let runtime = std::env::var_os("RL_TEST_APPIMAGE_RUNTIME").expect("Set RL_TEST_APPIMAGE_RUNTIME to the pinned Type-2 runtime (see docs/BUILDING.md)");
    let directory = appimage::tests::TempDir::new();
    builder_image(Path::new(&runtime), &directory.0.join("Test Builder.APPIMAGE"));
    // A separate process keeps Flatpak detection and the cache away from parallel tests and player data.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "prepare_appimage_tests::appimage_prepare_runs_from_disk_without_fuse", "--nocapture"])
        .env("RL_APPIMAGE_TEST_DIR", &directory.0)
        .env("FLATPAK_ID", "com.recomplauncher.app")
        .env("XDG_CACHE_HOME", directory.0.join("cache on disk"))
        .env("XDG_DATA_HOME", directory.0.join("data"))
        .env_remove("APPIMAGE_EXTRACT_AND_RUN")
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
