//! Embeds the Windows icon and version info into `tunebox.exe`.
//!
//! Works with any resource compiler that accepts MSVC `rc` arguments: `rc.exe` (Visual Studio),
//! `llvm-rc`, or Zig's `zig rc` (used when cross-compiling from Linux). Without one the build still
//! succeeds, the exe just has no file icon (the window icon is set at runtime regardless).

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows/tunebox.rc");
    println!("cargo:rerun-if-changed=assets/windows/tunebox.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/windows");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("tunebox.res");
    let _ = fs::remove_file(&out);

    let candidates: [&[&str]; 3] = [&["rc"], &["llvm-rc"], &["zig", "rc"]];
    for candidate in candidates {
        let status = Command::new(candidate[0])
            .args(&candidate[1..])
            .arg("/nologo")
            .arg("/fo")
            .arg(&out)
            .arg("tunebox.rc")
            .current_dir(&dir)
            .status();
        if matches!(status, Ok(s) if s.success()) && out.exists() {
            // Both MSVC's linker and lld accept compiled .res files on the command line.
            println!("cargo:rustc-link-arg-bins={}", out.display());
            return;
        }
    }
    println!("cargo:warning=no resource compiler (rc / llvm-rc / zig rc) found: tunebox.exe will have no file icon");
}
