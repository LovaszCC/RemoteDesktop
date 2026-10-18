//! Builds FreeRDP when its static build is missing (scripts/build-freerdp.sh or .ps1, pinned sources),
//! compiles the C shim against it and links everything statically.
//!
//! `RD_FREERDP_DIR` points at a prebuilt `{include,lib}` folder instead.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `canonicalize` gives Windows verbatim paths (`\\?\C:\...`), which the build tools (PowerShell, tar, cmake) mishandle.
fn plain(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = plain(manifest.join("../..").canonicalize().unwrap());
    let target = std::env::var("TARGET").unwrap();
    let windows = target.contains("windows");
    let script = root.join(if windows {
        "scripts/build-freerdp.ps1"
    } else {
        "scripts/build-freerdp.sh"
    });
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-env-changed=RD_FREERDP_DIR");
    let version = pinned_version(&root.join("scripts/build-freerdp.sh"));
    let options = root.join("scripts/freerdp-cmake-options.txt");
    println!("cargo:rerun-if-changed={}", options.display());
    let prebuilt = std::env::var_os("RD_FREERDP_DIR");
    let dir = prebuilt.as_ref().map_or_else(
        || {
            root.join("third_party/freerdp/build")
                .join(&version)
                .join(&target)
        },
        PathBuf::from,
    );
    // A build made with other options is rebuilt.
    let current = |path: &Path| std::fs::read(path).ok();
    let stale = !dir.join(".complete").exists() || current(&dir.join("OPTIONS")) != current(&options);
    if stale && prebuilt.is_none() {
        println!(
            "cargo:warning=building FreeRDP {version} for {target} (once; see {})",
            script.display()
        );
        let status = if windows {
            Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script)
                .args(["-Target", &target, "-Force"])
                .status()
        } else {
            Command::new("bash")
                .arg(&script)
                .arg(&target)
                .arg("--force")
                .status()
        };
        match status {
            Ok(status) if status.success() => {}
            other => panic!(
                "building FreeRDP failed ({other:?}); run {} {target} to see why",
                script.display()
            ),
        }
    }
    println!("cargo:rerun-if-changed={}", dir.join(".complete").display());
    println!("cargo:rustc-env=RD_FREERDP_VERSION={version}");

    let include = dir.join("include");
    let mut shim = cc::Build::new();
    if target.contains("apple") {
        // FreeRDP is built for the same minimum (scripts/build-freerdp.sh).
        shim.flag("-mmacosx-version-min=11.0");
    }
    if windows {
        // freerdp/api.h declares FREERDP_API dllimport unless FREERDP_EXPORTS is set, and the libraries
        // are static: without it every call links against a missing `__imp_` symbol.
        shim.define("FREERDP_EXPORTS", None);
    }
    shim.file(manifest.join("csrc/rdshim.c"))
        .include(include.join("freerdp3"))
        .include(include.join("winpr3"))
        .include(&include)
        .warnings(false)
        .compile("rdshim");

    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    // Static libraries in dependency order: the client library and its channel helpers first.
    let (prefix, suffix) = if windows { ("", ".lib") } else { ("lib", ".a") };
    let core = ["freerdp-client3", "freerdp3", "winpr3"];
    let crypto = if windows {
        ["libssl", "libcrypto"]
    } else {
        ["ssl", "crypto"]
    };
    let mut helpers: Vec<String> = std::fs::read_dir(&lib)
        .unwrap_or_else(|e| panic!("{}: {e}", lib.display()))
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_prefix(prefix)?.strip_suffix(suffix)?.to_owned();
            (!core.contains(&stem.as_str()) && !crypto.contains(&stem.as_str())).then_some(stem)
        })
        .collect();
    helpers.sort();
    println!("cargo:rustc-link-lib=static={}", core[0]);
    for helper in &helpers {
        println!("cargo:rustc-link-lib=static={helper}");
    }
    for name in core.iter().skip(1).chain(crypto.iter()) {
        println!("cargo:rustc-link-lib=static={name}");
    }
    system_libraries(&target);
}

/// `FREERDP_VERSION=…` from the build script, the single place the version is pinned.
fn pinned_version(script: &Path) -> String {
    std::fs::read_to_string(script)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("FREERDP_VERSION="))
        .expect("FREERDP_VERSION in scripts/build-freerdp.sh")
        .trim()
        .to_owned()
}

fn system_libraries(target: &str) {
    if target.contains("apple") {
        for framework in [
            "CoreFoundation",
            "CoreServices",
            "Security",
            "SystemConfiguration",
            "IOKit",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        println!("cargo:rustc-link-lib=dylib=z");
    } else if target.contains("windows") {
        for lib in [
            "ws2_32", "crypt32", "secur32", "ncrypt", "bcrypt", "advapi32", "user32", "shell32", "ole32",
            "shlwapi", "dbghelp", "userenv", "ntdsapi", "rpcrt4", "winmm", "credui", "gdi32",
        ] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    } else {
        for lib in ["dl", "pthread", "m", "rt", "z"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    }
}
