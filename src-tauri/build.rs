use sha2::{Digest, Sha256};
use std::{fs, path::Path};

/// The project credits are required. Fail the build when they, their page, or
/// the navigation entry that reaches them were removed.
fn require_credits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |relative: &str| {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
        fs::read_to_string(root.join(relative)).unwrap_or_default()
    };
    let credits = read("src/credits.rs");
    let region: String = credits
        .split("// CREDITS-BEGIN")
        .nth(1)
        .and_then(|rest| rest.split("// CREDITS-END").next())
        .map(|region| region.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    let digest: String = Sha256::digest(region.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let checks = [
        (digest == "8801985a94164e957710a3add0cdb6e170d56c5f31516e011ef11bae51044792", "src/credits.rs must contain the original credits"),
        (read("src/lib.rs").contains("credits::enforce()"), "src/lib.rs must call credits::enforce()"),
        (read("../src/lib/features/credits/CreditsPage.svelte").contains("get_credits"), "the Credits page is missing"),
        (read("../src/lib/navigation.ts").contains("\"credits\""), "the Credits navigation entry is missing"),
        (read("../src/App.svelte").contains("CreditsPage"), "the Credits page is not registered in App.svelte"),
    ];
    for (ok, message) in checks {
        if !ok {
            panic!("Credits are required and must not be removed: {message}.");
        }
    }
}

fn main() {
    require_credits();
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());

    let windows = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Library test executables need Common Controls v6 too.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app-manifest.xml"))
    };
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);

    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}
