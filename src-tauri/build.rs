fn main() {
    println!("cargo:rerun-if-changed=../.env");
    if let Ok(env) = std::fs::read_to_string("../.env") {
        for line in env.lines().map(str::trim) {
            if line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                println!("cargo:rustc-env={}={}", key.trim(), value.trim());
            }
        }
    }
    build_sidecar();
    tauri_build::build()
}

// Compiles the Swift transcriber that Tauri bundles as a sidecar (`externalBin`).
fn build_sidecar() {
    println!("cargo:rerun-if-changed=stt/main.swift");
    let target = std::env::var("TARGET").unwrap();
    let arch = target
        .split('-')
        .next()
        .unwrap()
        .replace("aarch64", "arm64");
    std::fs::create_dir_all("binaries").unwrap();
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "-swift-version", "5", "-target"])
        .arg(format!("{arch}-apple-macos26.0"))
        .args(["stt/main.swift", "-o"])
        .arg(format!("binaries/ease-stt-{target}"))
        .status()
        .expect("xcrun not found: install the Xcode command line tools");
    assert!(status.success(), "compiling stt/main.swift failed");
}
