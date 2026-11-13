mod engine_registry;

fn main() {
    println!("cargo:rerun-if-changed=src/engines/adapters");
    let registry = engine_registry::generate(std::path::Path::new("src/engines/adapters")).expect("engine adapters");
    std::fs::write(std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("engine-adapters.rs"), registry).unwrap();
    pack();
    // Cross-building for Windows with MinGW: unrar's C++ is built without a C++ library, so
    // libstdc++ goes in statically (the program needs no MinGW DLLs), with the Windows libraries
    // unrar names in #pragma comment, which MinGW doesn't read.
    if std::env::var("TARGET").unwrap_or_default().ends_with("windows-gnu") {
        if let Ok(out) = std::process::Command::new("x86_64-w64-mingw32-g++").arg("-print-file-name=libstdc++.a").output() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(dir) = std::path::Path::new(&p).parent().filter(|d| !d.as_os_str().is_empty()) {
                println!("cargo:rustc-link-search=native={}", dir.display());
            }
        }
        println!("cargo:rustc-link-lib=static=stdc++");
        // unrar_sys decides whether to build its Windows-only file by the machine it's built on,
        // so a build from Linux leaves it out: it's built here instead.
        if !cfg!(windows) {
            if let Some(dir) = unrar_src() {
                cc::Build::new()
                    .cpp(true)
                    .opt_level(2)
                    .std("c++14")
                    .cpp_link_stdlib(None)
                    .warnings(false)
                    .flag_if_supported("-Wno-unknown-pragmas")
                    .include(&dir)
                    .includes(std::env::var("CXXFLAGS_x86_64_pc_windows_gnu").ok().and_then(|f| f.strip_prefix("-I").map(str::to_string)))
                    .define("_FILE_OFFSET_BITS", Some("64"))
                    .define("_LARGEFILE_SOURCE", None)
                    .define("RAR_SMP", None)
                    .define("RARDLL", None)
                    .file(dir.join("isnt.cpp"))
                    .cargo_metadata(false)
                    .compile("unrar_isnt");
                // Whole, because it's linked before unrar_sys, which is what needs it.
                println!("cargo:rustc-link-search=native={}", std::env::var("OUT_DIR").unwrap_or_default());
                println!("cargo:rustc-link-lib=static:+whole-archive=unrar_isnt");
            }
        }
        for l in ["shlwapi", "wbemuuid", "ole32", "oleaut32", "powrprof"] {
            println!("cargo:rustc-link-lib={l}");
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        clang_runtime();
    }
    tauri_build::build()
}

/// SDL's Objective-C checks which macOS it runs on (`@available`) through clang's own runtime
/// library. clang adds that library itself, but Rust links with `-nodefaultlibs`, so it's named
/// here; without it a build for an older macOS than the SDK fails to link
/// (`___isPlatformVersionAtLeast`).
fn clang_runtime() {
    let cc = cc::Build::new().get_compiler();
    let dir = std::process::Command::new(cc.path()).arg("-print-resource-dir").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match dir.map(|d| std::path::Path::new(&d).join("lib/darwin/libclang_rt.osx.a")).filter(|p| p.is_file()) {
        Some(lib) => println!("cargo:rustc-link-arg={}", lib.display()),
        None => println!("cargo:warning=clang's runtime library (libclang_rt.osx.a) wasn't found; linking may fail"),
    }
}

/// unrar_sys's C++ sources, in Cargo's download folder.
fn unrar_src() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("CARGO_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cargo")))?;
    std::fs::read_dir(home.join("registry/src")).ok()?.flatten().find_map(|r| {
        let d = r.path().join("unrar_sys-0.5.8/vendor/unrar");
        d.join("isnt.cpp").is_file().then_some(d)
    })
}

/// The data the program carries (the shipped game profiles, each language's text), stored
/// compressed: a smaller program.
fn pack() {
    use std::io::Write;
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut files = vec![("boot/boot-profiles.json".to_string(), "boot-profiles.json.z".to_string())];
    for e in std::fs::read_dir("../ui/i18n").unwrap().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.ends_with(".json") {
            files.push((format!("../ui/i18n/{n}"), format!("i18n-{n}.z")));
        }
    }
    for (from, to) in files {
        println!("cargo:rerun-if-changed={from}");
        let data = std::fs::read(&from).unwrap();
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&data).unwrap();
        std::fs::write(out.join(to), z.finish().unwrap()).unwrap();
    }
    println!("cargo:rerun-if-changed=../ui/i18n");
}
