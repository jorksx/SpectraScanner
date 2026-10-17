use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=c_src/mbelib/");

    let out_dir = match env::var("OUT_DIR") {
        Ok(val) => val,
        Err(_) => return,
    };
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // 1. Compile mbelib C files into static library libmbe.a
    let c_dir = Path::new("c_src/mbelib");
    let mbelib_c = c_dir.join("mbelib.c");
    let ambe_c = c_dir.join("ambe3600x2450.c");
    let ecc_c = c_dir.join("ecc.c");

    let mbelib_o = Path::new(&out_dir).join("mbelib.o");
    let ambe_o = Path::new(&out_dir).join("ambe3600x2450.o");
    let ecc_o = Path::new(&out_dir).join("ecc.o");
    let lib_mbe = Path::new(&out_dir).join("libmbe.a");

    let cc = if target_os == "windows" { "gcc" } else { "cc" };
    let ar = if target_os == "windows" { "ar" } else { "ar" };

    let s1 = Command::new(cc)
        .args(["-O3", "-c"])
        .arg(&mbelib_c)
        .arg("-o")
        .arg(&mbelib_o)
        .status();

    let s2 = Command::new(cc)
        .args(["-O3", "-c"])
        .arg(&ambe_c)
        .arg("-o")
        .arg(&ambe_o)
        .status();

    let s3 = Command::new(cc)
        .args(["-O3", "-c"])
        .arg(&ecc_c)
        .arg("-o")
        .arg(&ecc_o)
        .status();

    if s1.map(|s| s.success()).unwrap_or(false)
        && s2.map(|s| s.success()).unwrap_or(false)
        && s3.map(|s| s.success()).unwrap_or(false)
    {
        let _ = Command::new(ar)
            .args(["rcs"])
            .arg(&lib_mbe)
            .arg(&mbelib_o)
            .arg(&ambe_o)
            .arg(&ecc_o)
            .status();
        println!("cargo:rustc-link-search=native={}", out_dir);
        println!("cargo:rustc-link-lib=static=mbe");
    }

    // 2. Windows System-DPI-aware manifest embedding:
    // Work around upstream rust-windowing/winit#4041 on Windows 11 where dragging
    // a window between monitors with different scale factors triggers an infinite
    // WM_DPICHANGED resize feedback loop that expands the window and snaps it back.
    if target_os == "windows" {
        let manifest_rc_path = Path::new(&out_dir).join("manifest.rc");
        let manifest_xml_path = Path::new(&out_dir).join("app.manifest");
        let manifest_obj_path = Path::new(&out_dir).join("manifest.o");

        let manifest_xml = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">system</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#;

        if fs::write(&manifest_xml_path, manifest_xml).is_ok() {
            let rc_content = format!("1 24 \"{}\"", manifest_xml_path.display().to_string().replace('\\', "/"));
            if fs::write(&manifest_rc_path, rc_content).is_ok() {
                let status = Command::new("windres")
                    .arg("-i")
                    .arg(&manifest_rc_path)
                    .arg("-o")
                    .arg(&manifest_obj_path)
                    .status();

                if let Ok(s) = status {
                    if s.success() {
                        println!("cargo:rustc-link-arg={}", manifest_obj_path.display());
                    }
                }
            }
        }
    }
}
