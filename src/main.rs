//! Entry point: the only place that reads the clock and the working directory.

#[tokio::main]
async fn main() {
    // Everything runs from the project root: the current folder when it is the project, otherwise
    // the project the executable sits in. Any other folder would get its own database and its own
    // STOP file, so the run refuses to start there.
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("Error: no se pudo leer la carpeta actual: {err}");
            std::process::exit(1);
        }
    };
    let exe = std::env::current_exe().ok();
    let Some(root) = jobbot::paths::find_root(&cwd, exe.as_deref()) else {
        eprintln!(
            "Error: {} no es la carpeta del proyecto (no tiene `profile/`). Ejecuta jobbot desde la carpeta del proyecto.",
            cwd.display()
        );
        std::process::exit(1);
    };
    let code = jobbot::cli::run(std::env::args().collect(), &root, chrono::Utc::now()).await;
    std::process::exit(code);
}
