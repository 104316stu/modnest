fn main() {
    tauri_build::build();
    test_manifest();
}

/// Test programs link Tauri too, and on Windows they won't start without the
/// Common Controls v6 manifest that the app itself gets. Give tests their own copy.
fn test_manifest() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
  </dependentAssembly></dependency>
</assembly>"#;
    std::fs::write(out.join("tests.manifest"), manifest).unwrap();
    if target_env == "msvc" {
        let m = out.join("tests.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", m.display());
        return;
    }
    // MinGW: compile the manifest into a resource object with windres.
    std::fs::write(out.join("tests.rc"), "1 24 \"tests.manifest\"\n").unwrap();
    let ok = std::process::Command::new("windres")
        .current_dir(&out)
        .args(["tests.rc", "-O", "coff", "-o", "tests-manifest.o"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-tests={}", out.join("tests-manifest.o").display());
    }
}
