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
    // JOBBOT_HOME, when set, holds all private state instead (profile, data, CV, STOP).
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
    };
    let home = var(jobbot::paths::HOME_VAR);
    let Some(root) = jobbot::paths::run_root(home.as_deref(), &cwd, exe.as_deref()) else {
        match &home {
            Some(home) => eprintln!(
                "Error: {} apunta a {}, que no tiene `profile/`. Corrige la variable o quítala.",
                jobbot::paths::HOME_VAR,
                home.display()
            ),
            None => eprintln!(
                "Error: {} no es la carpeta del proyecto (no tiene `profile/`). Ejecuta jobbot desde la carpeta del proyecto.",
                cwd.display()
            ),
        }
        std::process::exit(1);
    };
    let paths = jobbot::paths::Paths::for_run(&root, var("LOCALAPPDATA").as_deref());
    // Prints nothing unless JOBBOT_TRACE asks for the run's trace.
    jobbot::cli::init_trace(&root);
    let code = jobbot::cli::run(std::env::args().collect(), &paths, chrono::Utc::now()).await;
    std::process::exit(code);
}
