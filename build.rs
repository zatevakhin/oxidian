use std::path::Path;
use std::process::Command;

const UI_DIR: &str = "ui";

const WATCHED_PATHS: &[&str] = &[
    "src",
    "package.json",
    "bun.lock",
    "index.html",
    "vite.config.ts",
    "tsconfig.json",
    "postcss.config.js",
    "tailwind.config.js",
];

fn main() {
    for rel in WATCHED_PATHS {
        let watched = Path::new(UI_DIR).join(rel);
        if watched.exists() {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }

    let package_manager = detect_package_manager();
    let ui_path = Path::new(UI_DIR);

    if !ui_path.is_dir() {
        panic!("UI directory does not exist: {UI_DIR}");
    }

    run_cmd(package_manager, &["install"], UI_DIR);
    run_cmd(package_manager, &["run", "build"], UI_DIR);

    let dist = ui_path.join("dist");
    if !dist.is_dir() {
        panic!("UI build succeeded but dist/ missing ({})", dist.display());
    }
}

fn detect_package_manager() -> &'static str {
    if Command::new("bun").arg("--version").output().is_ok() {
        println!("cargo:warning=Detected bun - using it for UI build");
        "bun"
    } else if Command::new("npm").arg("--version").output().is_ok() {
        println!("cargo:warning=Detected npm - using it for UI build");
        "npm"
    } else {
        panic!("No package manager found. Install bun (preferred) or npm.");
    }
}

fn run_cmd(program: &str, args: &[&str], cwd: &str) {
    println!(
        "cargo:warning=Running '{} {}' in {cwd}",
        program,
        args.join(" "),
    );

    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "Failed to run '{} {}' in {cwd}: {err}",
                program,
                args.join(" "),
            )
        });

    if !status.success() {
        panic!(
            "Command failed in {cwd}: '{} {}' (exit: {status})",
            program,
            args.join(" "),
        );
    }
}
