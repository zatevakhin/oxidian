#[cfg(feature = "web-ui")]
use std::fs;
#[cfg(feature = "web-ui")]
use std::path::Path;
#[cfg(feature = "web-ui")]
use std::process::Command;

fn main() {
    // Only build UI when web-ui feature is enabled
    #[cfg(feature = "web-ui")]
    build_ui();
}

#[cfg(feature = "web-ui")]
fn get_package_manager() -> &'static str {
    if Command::new("bun").arg("--version").output().is_ok() {
        println!("cargo:warning=Detected bun - using it for UI build");
        "bun"
    } else if Command::new("npm").arg("--version").output().is_ok() {
        println!("cargo:warning=Detected npm - using it for UI build");
        "npm"
    } else {
        panic!("No package manager found. Please install bun (recommended) or npm.");
    }
}

#[cfg(feature = "web-ui")]
fn build_ui() {
    let ui_dir = Path::new("ui");

    // Rebuild if UI source files change
    println!("cargo:rerun-if-changed=ui/src");
    println!("cargo:rerun-if-changed=ui/package.json");
    println!("cargo:rerun-if-changed=ui/package-lock.json");
    println!("cargo:rerun-if-changed=ui/index.html");
    println!("cargo:rerun-if-changed=ui/vite.config.ts");
    println!("cargo:rerun-if-changed=ui/tsconfig.json");
    println!("cargo:rerun-if-changed=ui/postcss.config.js");
    println!("cargo:rerun-if-changed=ui/tailwind.config.js");

    // Support pre-built UI dist from Nix or CI
    println!("cargo:rerun-if-env-changed=OXI_UI_DIST");
    if let Ok(dist_path) = std::env::var("OXI_UI_DIST") {
        let dist_src = Path::new(&dist_path);
        if !dist_src.exists() {
            panic!("OXI_UI_DIST does not exist: {}", dist_path);
        }

        let dist_dst = ui_dir.join("dist");
        if dist_dst.exists() {
            fs::remove_dir_all(&dist_dst)
                .unwrap_or_else(|err| panic!("Failed to remove existing dist: {err}"));
        }
        copy_dir_all(dist_src, &dist_dst)
            .unwrap_or_else(|err| panic!("Failed to copy UI dist: {err}"));
        println!("cargo:warning=Using prebuilt UI from OXI_UI_DIST");
        return;
    }

    if !ui_dir.is_dir() {
        panic!("UI directory does not exist: ui");
    }

    let pm = get_package_manager();

    // Run install if node_modules doesn't exist or package.json changed
    if !ui_dir.join("node_modules").exists() {
        println!(
            "cargo:warning=Running {} install for UI dependencies...",
            pm
        );
        run_cmd(pm, &["install"], "ui");
    }

    // Run build
    println!("cargo:warning=Building UI with {}...", pm);
    run_cmd(pm, &["run", "build"], "ui");

    // Verify dist directory was created
    if !ui_dir.join("dist").exists() {
        panic!("UI build succeeded but dist/ directory not found");
    }

    println!("cargo:warning=UI build complete!");
}

#[cfg(feature = "web-ui")]
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

#[cfg(feature = "web-ui")]
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
