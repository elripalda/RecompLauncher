use recomplauncher_lib::engines::coordinator::copy_tree;
use std::{cell::Cell, fs, path::PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rl-engine-copy-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("source/original"), "keep this input").unwrap();
    root
}

#[test]
fn copy_rejects_a_new_destination_inside_its_source_before_writing() {
    let root = scratch("descendant");
    let source = root.join("source");
    let destination = source.join("work/output");
    // Bound the old recursive behavior so a regression cannot fill the test machine's disk.
    let calls = Cell::new(0);
    let result = copy_tree(&source, &destination, &|| { calls.set(calls.get() + 1); calls.get() > 8 });
    let destination_created = destination.exists();
    fs::remove_dir_all(root).unwrap();
    assert!(result.unwrap_err().contains("inside"));
    assert!(!destination_created);
}

#[test]
fn copy_cannot_overwrite_the_source_itself() {
    let root = scratch("same");
    let source = root.join("source");
    let result = copy_tree(&source, &source, &|| false);
    let original = fs::read_to_string(source.join("original")).unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(result.is_err());
    assert_eq!(original, "keep this input");
}

#[test]
fn sibling_output_copy_still_works() {
    let root = scratch("sibling");
    copy_tree(&root.join("source"), &root.join("work/output"), &|| false).unwrap();
    assert_eq!(fs::read_to_string(root.join("work/output/original")).unwrap(), "keep this input");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn destination_through_a_directory_link_cannot_hide_a_descendant() {
    let root = scratch("alias");
    std::os::unix::fs::symlink(root.join("source"), root.join("alias")).unwrap();
    let calls = Cell::new(0);
    let result = copy_tree(&root.join("source"), &root.join("alias/work/output"), &|| { calls.set(calls.get() + 1); calls.get() > 8 });
    let destination_created = root.join("source/work").exists();
    fs::remove_dir_all(root).unwrap();
    assert!(result.unwrap_err().contains("inside"));
    assert!(!destination_created);
}
