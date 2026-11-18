//! Command line interface: a thin layer that parses arguments, applies the safety switches
//! (STOP file, execution window) and calls the other modules. Messages for Gonza are in Spanish.
//!
//! Exit codes: 0 ok, 1 error (including a usage error), 2 CV not ready, 3 STOP file present,
//! 4 outside the execution window.
//!
//! This file says which commands exist and how each one is dispatched; the body of each lives in
//! the module of its topic, next to its tests.

mod claude;
mod daily;
mod dossier;
mod freelance;
mod inbox;
mod marks;
mod panel;
mod pipeline;
mod prep;
mod reports;
mod run_guard;
mod submit;
#[cfg(test)]
mod test_support;
mod trace;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use jobbot_app::privacy::UserPaths;

use self::claude::{cmd_claude_check, cmd_claude_login};
use self::daily::cmd_run_daily;
use self::dossier::cmd_dossier;
use self::freelance::cmd_freelance;
use self::inbox::{cmd_inbox_import, cmd_task_add, cmd_task_done};
use self::marks::{cmd_approve, cmd_confirm, cmd_mark, cmd_record, cmd_resolve, cmd_unmark};
use self::panel::cmd_panel;
use self::pipeline::{
    cmd_auto_approve, cmd_discover, cmd_install_browsers, cmd_prepare, cmd_prepare_refresh,
};
use self::prep::cmd_prep;
use self::reports::{cmd_digest, cmd_results, cmd_status, cmd_today, cmd_top};
use self::run_guard::{cmd_resume, cmd_stop, start_run};
use self::submit::cmd_submit;
use self::trace::{PlainText, log_output};
use crate::claude::ClaudeRunner;
use crate::cv::{BrowserRenderer, Renderer};
use crate::form::{FormFiller, SubmitMode};
use crate::handoff::FormInspector;
use crate::open_check::OpenChecker;
use crate::paths::Paths;
use crate::sources::Source;
use crate::tracker::Tracker;

pub use self::trace::{TRACE_VAR, init_trace};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_CV_NOT_READY: i32 = 2;
pub const EXIT_STOP: i32 = 3;
pub const EXIT_OUTSIDE_WINDOW: i32 = 4;

#[derive(Parser, Debug)]
#[command(
    name = "jobbot",
    about = "Automatización de búsqueda de empleo de Gonza"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Flag shared by every command that changes something.
#[derive(Args, Debug, Clone, Copy)]
struct WindowArgs {
    /// Corre aunque sea horario laboral de Gonza. No salta el archivo STOP.
    #[arg(long)]
    ignore_window: bool,
}

#[derive(Args, Debug, Clone, Copy)]
struct PrepareArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Vuelve a generar, en su mismo lugar, el CV de lo preparado y de lo que espera tu decisión, desde el CV maestro de hoy (tras editarlo). No cambia estados; lo enviado nunca se toca, y el PDF anterior se guarda en data/cv/replaced.
    #[arg(long)]
    refresh: bool,
}

#[derive(Args, Debug, Clone, Copy)]
struct DiscoverArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Vuelve a puntuar los avisos guardados que no tienen postulación (tras cambiar criteria.toml).
    #[arg(long)]
    rescore: bool,
}

#[derive(Args, Debug, Clone, Copy)]
struct SubmitArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// Muestra el navegador mientras llena los formularios (--assist lo implica).
    #[arg(long)]
    headed: bool,
    /// Envía de verdad. Sin este flag solo se llena el formulario y se guarda una captura.
    #[arg(long, conflicts_with = "assist")]
    live: bool,
    /// Modo asistido: llena en un navegador visible lo que sabe responder, te deja marcado en rojo lo
    /// que falta (preguntas abiertas, datos que no tiene) y tú lo completas y haces clic en enviar.
    #[arg(long)]
    assist: bool,
}

impl SubmitArgs {
    fn mode(&self) -> SubmitMode {
        if self.assist {
            SubmitMode::Assist
        } else if self.live {
            SubmitMode::Live
        } else {
            SubmitMode::DryRun
        }
    }
}

#[derive(Args, Debug, Clone)]
struct DossierArgs {
    #[command(flatten)]
    window: WindowArgs,
    #[arg(required = true)]
    keys: Vec<String>,
}

#[derive(Args, Debug, Clone)]
struct RecordArgs {
    /// Clave de un aviso que ya está en la base (la ves con `top`). Sin ella usa --company, --title y --url.
    #[arg(long, conflicts_with_all = ["company", "title", "url"])]
    key: Option<String>,
    #[arg(long)]
    company: Option<String>,
    #[arg(long)]
    title: Option<String>,
    /// Enlace del aviso o del formulario.
    #[arg(long)]
    url: Option<String>,
    #[arg(long)]
    location: Option<String>,
    /// El CV que se envió.
    #[arg(long)]
    cv: Option<PathBuf>,
    /// Cuándo se envió, en formato RFC 3339 (por ejemplo 2026-10-05T05:21:42Z). Por defecto, ahora.
    #[arg(long)]
    at: Option<String>,
    #[arg(long)]
    note: Option<String>,
    /// Sueldo mínimo publicado, en USD al mes.
    #[arg(long)]
    salary_min: Option<f64>,
    /// Sueldo máximo publicado, en USD al mes.
    #[arg(long)]
    salary_max: Option<f64>,
}

#[derive(Args, Debug, Clone, Copy)]
struct PanelArgs {
    /// Puerto en 127.0.0.1. Sin esta opción prueba el 47213 (así el navegador recuerda idioma, tema
    /// y sonido, que se guardan por puerto) y, si está ocupado, usa uno libre; 0 = siempre uno libre.
    #[arg(long)]
    port: Option<u16>,
    /// No abrir el navegador: solo imprimir el enlace.
    #[arg(long)]
    no_open: bool,
}

#[derive(Args, Debug, Clone)]
struct FreelanceProfileArgs {
    #[command(flatten)]
    window: WindowArgs,
    /// La plataforma (debe estar en `platforms` de profile/freelance.toml).
    platform: String,
}

#[derive(Args, Debug, Clone)]
struct FreelanceProposalArgs {
    #[command(flatten)]
    window: WindowArgs,
    platform: String,
    /// Archivo de texto con el aviso (pégalo ahí: el programa no navega plataformas).
    #[arg(long)]
    job_file: PathBuf,
}

#[derive(Subcommand, Debug)]
enum FreelanceCommand {
    /// Redacta un borrador de perfil (titular, bio, habilidades, tarifa, ideas de portafolio).
    Profile(FreelanceProfileArgs),
    /// Redacta un borrador de propuesta para el aviso que guardaste en un archivo.
    Proposal(FreelanceProposalArgs),
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Busca avisos, filtra, puntúa y deja la cola (--rescore repuntúa lo guardado antes).
    Discover(DiscoverArgs),
    /// Adapta el CV de cada aviso en cola y genera los PDF.
    Prepare(PrepareArgs),
    /// Llena los formularios (prueba en seco), envía (--live) o te deja lo que falta y el clic a ti (--assist).
    Submit(SubmitArgs),
    /// Aprueba postulaciones probadas en seco.
    Approve {
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Devuelve a la cola postulaciones que estaban en "necesita a Gonza" (a puntuadas si no tienen PDF).
    Resolve {
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Marca como enviada una postulación en "necesita a Gonza" que enviaste tú o que sí llegó.
    Confirm {
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Escribe el resumen diario en español (también se guarda en data/).
    Digest,
    /// Lista los mejores puntajes.
    Top {
        /// Cuántos avisos mostrar.
        #[arg(long, default_value_t = 20)]
        n: usize,
    },
    /// Cuenta las postulaciones por estado.
    Status,
    /// Muestra lo último que hizo el envío (data/results.jsonl): modo, estado, preguntas llenadas y lo que faltó.
    Results {
        /// Cuántas líneas mostrar (las más recientes).
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..))]
        last: u64,
    },
    /// Muestra, fija o apaga el puntaje desde el que el motor podrá aprobar sin ti (un entero desde el
    /// puntaje mínimo para postular hasta 100, u `off`). Solo escribe en la base: ignora STOP y la ventana.
    AutoApprove {
        /// Un entero, `off` para apagarla, o nada para ver el valor de ahora.
        #[arg(allow_hyphen_values = true)]
        value: Option<String>,
    },
    /// Crea el archivo STOP: detiene discover, prepare, submit, run-daily, dossier, freelance y claude-check.
    Stop,
    /// Borra el archivo STOP.
    Resume,
    /// discover + prepare + submit + digest (lo que corre el Programador de tareas).
    RunDaily(WindowArgs),
    /// Instala Chromium para playwright-rs (una sola vez).
    InstallBrowsers,
    /// Comprueba que Claude responde (una llamada mínima) y muestra versión, cuenta, costo y presupuesto.
    ClaudeCheck(WindowArgs),
    /// Inicia sesión una vez con tu cuenta personal de Claude en el directorio propio de jobbot (abre el navegador).
    ClaudeLogin,
    /// Escribe el dossier de preparación de un aviso (llama a Claude; los huecos pasan a tarjetas de repaso).
    Dossier(DossierArgs),
    /// Registra a mano una respuesta: replied, technical_test, interview, offer, accepted, rejected, withdrawn o ghosted.
    Mark {
        key: String,
        state: String,
        /// Una nota corta (por ejemplo la fecha de la entrevista).
        #[arg(long)]
        note: Option<String>,
    },
    /// Deshace tu última marca de una postulación, dentro de 10 minutos y si nada más la cambió después.
    Unmark { key: String },
    /// Registra una postulación que se envió a mano (tú, o Claude con Playwright): queda como enviada con su hora real.
    Record(RecordArgs),
    /// Bandeja de correo: importa los correos que Claude leyó de tu Gmail (jobbot nunca toca Gmail).
    Inbox {
        #[command(subcommand)]
        command: InboxCommand,
    },
    /// Tu lista de hoy: tareas abiertas, lo que espera tu decisión y las respuestas recientes. Solo lee.
    Today,
    /// Tus tareas (las que dejan los correos importados).
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Borradores freelance (perfil y propuestas). Nada se envía a ninguna plataforma.
    Freelance {
        #[command(subcommand)]
        command: FreelanceCommand,
    },
    /// Repaso espaciado de preguntas de entrevista.
    Prep {
        #[command(subcommand)]
        command: PrepCommand,
    },
    /// Abre el panel local (postulaciones y repaso) en tu navegador. Solo este PC puede verlo.
    Panel(PanelArgs),
}

#[derive(Subcommand, Debug)]
enum InboxCommand {
    /// Importa un archivo JSON con los correos (una lista de mensajes). Importar el mismo archivo
    /// dos veces no cambia nada. Solo escribe en la base: ignora STOP y la ventana.
    Import {
        /// El archivo JSON (ver «Bandeja de correo» en el README).
        file: PathBuf,
    },
}

#[derive(Args, Debug, Clone)]
struct TaskAddArgs {
    /// Qué tienes que hacer (se corta a 300 caracteres).
    text: String,
    /// Fecha límite en formato RFC 3339 (por ejemplo 2026-10-08T15:00:00Z o 2026-10-08T12:00:00-03:00).
    #[arg(long)]
    due: Option<String>,
    /// La clave del aviso del que se trata (debe estar en la base; la ves con `top`).
    #[arg(long)]
    job_key: Option<String>,
    /// Una dirección http o https para abrir (hasta 300 caracteres).
    #[arg(long)]
    link: Option<String>,
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    /// Agrega una tarea a tu lista de hoy, sin necesidad de un correo. Solo escribe en la base:
    /// ignora STOP y la ventana. La misma tarea (mismo texto y aviso) no se agrega dos veces
    /// mientras siga abierta.
    Add(TaskAddArgs),
    /// Marca una tarea como hecha (el número sale en `jobbot today`).
    Done {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        id: i64,
    },
}

#[derive(Subcommand, Debug)]
enum PrepCommand {
    /// Muestra la siguiente pregunta (primero las vencidas, luego las nuevas).
    Next {
        /// Solo preguntas de este tema (por ejemplo javascript, sql, rust).
        #[arg(long)]
        topic: Option<String>,
    },
    /// Muestra la respuesta de referencia de una pregunta.
    Reveal { id: String },
    /// Registra tu nota de una pregunta, de 0 a 5 (menos de 3 vuelve mañana).
    Grade {
        id: String,
        #[arg(value_parser = clap::value_parser!(u8).range(0..=5))]
        grade: u8,
    },
    /// Cuenta preguntas vistas y pendientes, y lista los temas débiles.
    Stats,
}

/// Everything the commands need from the outside world. Tests build one with fakes, so no
/// command test touches the network or a browser. `None` means "build the real one when needed".
pub struct Deps {
    pub sources: Option<Vec<Box<dyn Source>>>,
    pub renderer: Box<dyn Renderer>,
    pub filler: Option<Box<dyn FormFiller>>,
    /// Opens the handoff forms (Ashby) and lists their questions. `None` means: start a headless
    /// Chromium of its own when a run has such a form, unless `filler` is injected (a test), in
    /// which case the handoff forms are dry-run through that filler instead.
    pub inspector: Option<Box<dyn FormInspector>>,
    /// Asks each posting's source whether it is still open before `submit` applies. `None` means:
    /// the real network checker, unless `filler` is injected (a test), which means no checks.
    pub open_checker: Option<Box<dyn OpenChecker>>,
    /// `None` means: find the real `claude.exe` when a command needs it.
    pub claude: Option<Arc<dyn ClaudeRunner>>,
}

impl Deps {
    /// Real sources (from `profile/sources.toml`) and a real browser filler are created lazily
    /// by the commands that need them, so `status` or `stop` never start a browser.
    pub fn real() -> Deps {
        Deps {
            sources: None,
            renderer: Box::new(BrowserRenderer),
            filler: None,
            inspector: None,
            open_checker: None,
            claude: None,
        }
    }
}

/// Entry point used by `main`: the run's layout (`Paths::for_run`), real dependencies, output on
/// stdout. `args` includes the program name as its first element, exactly like
/// `std::env::args().collect()`.
pub async fn run(args: Vec<String>, paths: &Paths, now: DateTime<Utc>) -> i32 {
    let stdout = std::io::stdout();
    let terminal = stdout.is_terminal();
    let mut out = log_output(stdout, terminal, || {
        UserPaths::for_profile(&paths.profile_dir)
    });
    run_in(args, paths, now, Deps::real(), &mut out).await
}

/// Same as `run` with the layout of `root`, injected dependencies and output: what the tests call.
pub async fn run_with(
    args: Vec<String>,
    root: &Path,
    now: DateTime<Utc>,
    deps: Deps,
    out: &mut dyn Write,
) -> i32 {
    run_in(args, &Paths::new(root), now, deps, out).await
}

async fn run_in(
    args: Vec<String>,
    paths: &Paths,
    now: DateTime<Utc>,
    deps: Deps,
    out: &mut dyn Write,
) -> i32 {
    let mut plain = PlainText::new(out);
    let out: &mut dyn Write = &mut plain;
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // `--help` and `--version` are not failures; a usage error is.
            let _ = write!(out, "{}", err.render());
            return if err.use_stderr() {
                EXIT_ERROR
            } else {
                EXIT_OK
            };
        }
    };
    match dispatch(cli.command, paths, now, deps, out).await {
        Ok(code) => code,
        Err(err) => {
            let _ = writeln!(out, "Error: {err:#}");
            EXIT_ERROR
        }
    }
}

async fn dispatch(
    command: Command,
    paths: &Paths,
    now: DateTime<Utc>,
    deps: Deps,
    out: &mut dyn Write,
) -> Result<i32> {
    match command {
        Command::Discover(args) => {
            let _run = match start_run(paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_discover(paths, &deps, now, args.rescore, out).await
        }
        Command::Prepare(args) => {
            let _run = match start_run(paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            if args.refresh {
                cmd_prepare_refresh(paths, &deps, now, out).await
            } else {
                cmd_prepare(paths, &deps, now, out).await
            }
        }
        Command::Submit(args) => {
            let run = match start_run(paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            // Assist needs a window Gonza can see.
            cmd_submit(
                paths,
                &deps,
                now,
                args.headed || args.assist,
                args.mode(),
                &run.watch,
                out,
            )
            .await
        }
        Command::RunDaily(w) => {
            let run = match start_run(paths, now, w.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_run_daily(paths, &deps, now, &run.watch, out).await
        }
        Command::ClaudeCheck(w) => {
            let _run = match start_run(paths, now, w.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_claude_check(paths, &deps, now, out).await
        }
        // Setup Gonza does once, by hand: it ignores STOP and the window like `mark` does.
        Command::ClaudeLogin => cmd_claude_login(paths, &deps, now, out).await,
        Command::Dossier(args) => {
            let _run = match start_run(paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_dossier(paths, &deps, now, &args.keys, out).await
        }
        Command::Mark { key, state, note } => {
            cmd_mark(paths, &key, &state, note.as_deref(), now, out)
        }
        Command::Unmark { key } => cmd_unmark(paths, &key, now, out),
        Command::Record(args) => cmd_record(paths, args, now, out),
        Command::Inbox { command } => match command {
            InboxCommand::Import { file } => cmd_inbox_import(paths, &file, now, out),
        },
        Command::Today => cmd_today(paths, now, out),
        Command::Task { command } => match command {
            TaskCommand::Add(args) => cmd_task_add(paths, args, now, out),
            TaskCommand::Done { id } => cmd_task_done(paths, id, out),
        },
        Command::Freelance { command } => {
            let ignore_window = match &command {
                FreelanceCommand::Profile(a) => a.window.ignore_window,
                FreelanceCommand::Proposal(a) => a.window.ignore_window,
            };
            let _run = match start_run(paths, now, ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_freelance(paths, &deps, now, command, out).await
        }
        Command::Approve { keys } => cmd_approve(paths, &keys, out),
        Command::Resolve { keys } => cmd_resolve(paths, &keys, now, out),
        Command::Confirm { keys } => cmd_confirm(paths, &keys, now, out),
        Command::Digest => cmd_digest(paths, now, out),
        Command::Top { n } => cmd_top(paths, n, out),
        Command::Status => cmd_status(paths, now, out),
        Command::Results { last } => cmd_results(paths, last, out),
        Command::AutoApprove { value } => cmd_auto_approve(paths, value.as_deref(), out),
        Command::Stop => cmd_stop(paths, out),
        Command::Resume => cmd_resume(paths, out),
        Command::InstallBrowsers => cmd_install_browsers(out).await,
        Command::Prep { command } => cmd_prep(paths, command, now, out),
        Command::Panel(args) => cmd_panel(paths, now, args, out).await,
    }
}

fn open_tracker(paths: &Paths) -> Result<Tracker> {
    std::fs::create_dir_all(&paths.data_dir)?;
    Ok(Tracker::open(&paths.data_dir.join("jobbot.db"))?)
}

#[cfg(test)]
mod tests {
    use super::test_support::{open_now, plain, temp_root};

    #[tokio::test]
    async fn usage_errors_and_help() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["frobnicate"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("error"), "{out}");
        let (code, out) = plain(root.path(), open_now(), &["--help"]).await;
        assert_eq!(code, 0);
        assert!(
            out.contains("run-daily")
                && out.contains("install-browsers")
                && out.contains("confirm"),
            "{out}"
        );
        // approve and confirm need at least one key.
        assert_eq!(plain(root.path(), open_now(), &["approve"]).await.0, 1);
        assert_eq!(plain(root.path(), open_now(), &["confirm"]).await.0, 1);
    }

    #[tokio::test]
    async fn the_main_help_lists_the_panel() {
        let root = temp_root();
        let (_, out) = plain(root.path(), open_now(), &["--help"]).await;
        assert!(out.contains("panel"), "{out}");
    }
}
