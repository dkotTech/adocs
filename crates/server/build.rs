// Builds the frontend before compiling: frontend/dist is embedded into the binary.
// Documentation is not part of this, it is supplied as an archive at run time.
// The frontend build can be skipped with SKIP_FRONTEND_BUILD=1.
use std::path::Path;
use std::process::Command;

fn main() {
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend");
    for f in [
        "src",
        "index.html",
        "package.json",
        "package-lock.json",
        "vite.config.ts",
        "tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={}", frontend.join(f).display());
    }
    println!("cargo:rerun-if-env-changed=SKIP_FRONTEND_BUILD");

    if std::env::var_os("SKIP_FRONTEND_BUILD").is_some() {
        return;
    }
    if !frontend.join("node_modules").exists() {
        run(&frontend, &["ci"]);
    }
    run(&frontend, &["run", "build"]);
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("npm")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to run `npm {}`: {e}. \
                 Set SKIP_FRONTEND_BUILD=1 to use an already built frontend/dist",
                args.join(" ")
            )
        });
    if !status.success() {
        panic!("`npm {}` failed in {}", args.join(" "), dir.display());
    }
}
