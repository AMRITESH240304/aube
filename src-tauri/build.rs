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
    build_sidecar(&["stt/main.swift"], "aube-stt");
    build_sidecar(&["shared/Ollama.swift", "notes/main.swift"], "aube-notes");
    build_sidecar(
        &[
            "shared/Ollama.swift",
            "dictate/Commands.swift",
            "dictate/main.swift",
        ],
        "aube-dictate",
    );
    build_diarizer();
    fetch_speaker_models();
    fetch_language_model();
    tauri_build::build()
}

const LANGUAGE_MODEL: &str =
    "https://huggingface.co/krut42/voice-lid-ecapa-voxlingua107/resolve/f050aac33e6908a2da7a74f6dde9790b79b69673";

fn fetch_language_model() {
    fetch(
        "models/lid-ecapa-voxlingua107.int8.onnx",
        "3f1504000353f1da5015ae0df0bdae8d1fd7064680e45a8c9cb02740d78c4fa9",
    );
    fetch(
        "models/lid-labels.txt",
        "8be3791b6a1dfe2db769f32e93d6e7571a42662f3336a33ad837409745d5af04",
    );
}

fn fetch(path: &str, sha256: &str) {
    println!("cargo:rerun-if-changed={path}");
    if sha256_of(path).as_deref() == Some(sha256) {
        return;
    }
    let name = path.trim_start_matches("models/");
    std::fs::create_dir_all("models").unwrap();
    let status = std::process::Command::new("curl")
        .args(["-sfL", "-o", path])
        .arg(format!("{LANGUAGE_MODEL}/{name}"))
        .status()
        .expect("curl not found");
    assert!(status.success(), "downloading {name} failed");
    assert_eq!(
        sha256_of(path).as_deref(),
        Some(sha256),
        "{name} doesn't match its checksum"
    );
}

fn sha256_of(path: &str) -> Option<String> {
    let output = std::process::Command::new("shasum")
        .args(["-a", "256", path])
        .output()
        .ok()?;
    let digest = String::from_utf8(output.stdout).ok()?;
    output.status.success().then(|| {
        digest
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

// Compiles a Swift sidecar that Tauri bundles (`externalBin`).
fn build_sidecar(sources: &[&str], name: &str) {
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    let target = std::env::var("TARGET").unwrap();
    let arch = target
        .split('-')
        .next()
        .unwrap()
        .replace("aarch64", "arm64");
    std::fs::create_dir_all("binaries").unwrap();
    let mut command = std::process::Command::new("xcrun");
    command
        .args(["swiftc", "-O", "-swift-version", "5", "-target"])
        .arg(format!("{arch}-apple-macos26.0"));
    command.args(sources);
    let status = command
        .arg("-o")
        .arg(format!("binaries/{name}-{target}"))
        .status()
        .expect("xcrun not found: install the Xcode command line tools");
    assert!(status.success(), "compiling {sources:?} failed");
}

fn build_diarizer() {
    println!("cargo:rerun-if-changed=diarizer/Package.swift");
    println!("cargo:rerun-if-changed=diarizer/Sources");
    let target = std::env::var("TARGET").unwrap();
    let arch = target
        .split('-')
        .next()
        .unwrap()
        .replace("aarch64", "arm64");
    let swift_build = |extra: &[&str]| {
        std::process::Command::new("xcrun")
            .args([
                "swift",
                "build",
                "-c",
                "release",
                "--package-path",
                "diarizer",
                "--arch",
            ])
            .arg(&arch)
            .args(extra)
            .output()
            .expect("xcrun not found: install the Xcode command line tools")
    };
    let built = swift_build(&[]);
    assert!(
        built.status.success(),
        "building the diarizer failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let bin = String::from_utf8(swift_build(&["--show-bin-path"]).stdout).unwrap();
    std::fs::copy(
        std::path::Path::new(bin.trim()).join("aube-diarizer"),
        format!("binaries/aube-diarizer-{target}"),
    )
    .expect("copying the diarizer failed");
}

const SPEAKER_MODELS: &str = "models/diarizer";

fn fetch_speaker_models() {
    let marker = format!("{SPEAKER_MODELS}/speaker-diarization/.fluidaudio-revision");
    println!("cargo:rerun-if-changed={marker}");
    if std::path::Path::new(&marker).exists() {
        return;
    }
    let target = std::env::var("TARGET").unwrap();
    let output = std::process::Command::new(format!("binaries/aube-diarizer-{target}"))
        .args(["--fetch", SPEAKER_MODELS])
        .output()
        .expect("running the diarizer failed");
    assert!(
        output.status.success(),
        "downloading the speaker models failed:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
