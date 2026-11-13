use std::{fs, path::Path};

/// Adapter folders register themselves. Sorting keeps builds and catalog order reproducible.
pub fn generate(root: &Path) -> std::io::Result<String> {
    let mut folders = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    folders.sort_by_key(|e| e.file_name());
    let mut modules = String::new();
    let mut entries = Vec::new();
    for folder in folders {
        if !folder.file_type()?.is_dir() { continue; }
        let source = folder.path().join("mod.rs");
        if !source.is_file() { continue; }
        let id = folder.file_name().to_string_lossy().to_string();
        assert!(!id.is_empty() && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') && !id.as_bytes()[0].is_ascii_digit(), "Invalid engine folder: {id}");
        modules.push_str(&format!("#[path = {:?}] pub mod {id};\n", source.canonicalize()?));
        entries.push(format!("&{id}::ENGINE"));
    }
    modules.push_str(&format!("pub static ADAPTERS: &[&dyn registry::Adapter] = &[{}];\n", entries.join(", ")));
    Ok(modules)
}
