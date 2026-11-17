//! Schreibt Git-Branch und Commit in die Binärdatei, damit die Oberfläche bei Fehlern den
//! gebauten Stand zeigt. Ohne Git (z. B. Quellarchiv) steht dort „unbekannt“.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unbekannt".into());
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unbekannt".into());
    // Ungesicherte Änderungen im Quellbaum markieren („+“), damit ein lokaler Zwischenstand
    // nicht mit dem Commit verwechselt wird.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some();
    println!("cargo:rustc-env=FOTO_IMPORTER_GIT_BRANCH={branch}");
    println!(
        "cargo:rustc-env=FOTO_IMPORTER_GIT_COMMIT={commit}{}",
        if dirty { "+" } else { "" }
    );
    // Neu ausführen bei Commit, Checkout oder Änderungen im Quellbaum.
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for file in ["HEAD", "index", "logs/HEAD"] {
            println!("cargo:rerun-if-changed={dir}/{file}");
        }
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}
