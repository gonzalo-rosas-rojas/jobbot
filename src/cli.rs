//! Command line interface: a thin layer that parses arguments, applies the safety switches
//! (STOP file, execution window) and calls the other modules. Messages for Gonza are in Spanish.
//!
//! Exit codes: 0 ok, 1 error (including a usage error), 2 CV not ready, 3 STOP file present,
//! 4 outside the execution window.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use clap::{Args, Parser, Subcommand};

use crate::actions;
use crate::answers::load_answers;
use crate::claude::{
    ClaudeConfig, ClaudeGate, ClaudeRequest, ClaudeRunner, CliClaude, TESTED_CLAUDE_VERSION,
    is_account_error, is_budget_error, load_claude_config,
};
use crate::cv::{BrowserRenderer, CvError, Renderer};
use crate::digest::build_digest;
use crate::discovery::{DiscoveryReport, discover, rescore};
use crate::dossier::{build_dossier, dossier_path};
use crate::form::{BrowserFiller, FormFiller, SubmitMode, SubmitResult};
use crate::freelance::{draft_profile, draft_proposal, load_freelance_config};
use crate::handoff::{BrowserInspector, FormInspector, is_handoff_job, is_handoff_reason};
use crate::inbox;
use crate::models::Job;
use crate::open_check::{NetworkOpenChecker, OpenChecker};
use crate::outbound;
use crate::panel::{Panel, scheduler::WindowsTasks, server::PanelServer};
use crate::paths::Paths;
use crate::pipeline::{prepare_report, submit_all_while};
use crate::prep::PrepStore;
use crate::results::{format_table, read_last, results_path};
use crate::rules::{load_criteria, load_exclusions};
use crate::sources::{Source, build_sources, load_sources_config};
use crate::today::build_today;
use crate::tracker::{ApplicationRow, State, Tracker};
use crate::window::{Schedule, in_run_window, load_schedule, next_window_start};

/// `run-daily` fires every hour, but the boards do not change that fast: discovery is skipped
/// while the last one (`meta.last_discover_at`) is younger than this. `jobbot discover` itself
/// always runs. Six hours keeps the scheduled runs to four a day at most: Remotive's terms ask
/// for no more than four requests a day, and the other boards do not change faster than that.
const DISCOVER_COOLDOWN_HOURS: i64 = 6;

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

/// The port `jobbot panel` asks for first (see `PanelArgs::port`).
const PREFERRED_PANEL_PORT: u16 = 47213;

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
    Prepare(WindowArgs),
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
        /// El archivo JSON (ver «Inbox file» en el README).
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

/// Entry point used by `main`: real dependencies, output on stdout. `args` includes the
/// program name as its first element, exactly like `std::env::args().collect()`.
pub async fn run(args: Vec<String>, root: &Path, now: DateTime<Utc>) -> i32 {
    run_with(args, root, now, Deps::real(), &mut std::io::stdout()).await
}

/// Same as `run` with injected dependencies and output, which is what the tests call.
pub async fn run_with(
    args: Vec<String>,
    root: &Path,
    now: DateTime<Utc>,
    deps: Deps,
    out: &mut dyn Write,
) -> i32 {
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
    match dispatch(cli.command, root, now, deps, out).await {
        Ok(code) => code,
        Err(err) => {
            let _ = writeln!(out, "Error: {err:#}");
            EXIT_ERROR
        }
    }
}

async fn dispatch(
    command: Command,
    root: &Path,
    now: DateTime<Utc>,
    deps: Deps,
    out: &mut dyn Write,
) -> Result<i32> {
    let paths = Paths::new(root);
    match command {
        Command::Discover(args) => {
            let _run = match start_run(&paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_discover(&paths, &deps, now, args.rescore, out).await
        }
        Command::Prepare(w) => {
            let _run = match start_run(&paths, now, w.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_prepare(&paths, &deps, now, out).await
        }
        Command::Submit(args) => {
            let run = match start_run(&paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            // Assist needs a window Gonza can see.
            cmd_submit(
                &paths,
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
            let run = match start_run(&paths, now, w.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_run_daily(&paths, &deps, now, &run.watch, out).await
        }
        Command::ClaudeCheck(w) => {
            let _run = match start_run(&paths, now, w.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_claude_check(&paths, &deps, now, out).await
        }
        // Setup Gonza does once, by hand: it ignores STOP and the window like `mark` does.
        Command::ClaudeLogin => cmd_claude_login(&paths, &deps, now, out).await,
        Command::Dossier(args) => {
            let _run = match start_run(&paths, now, args.window.ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_dossier(&paths, &deps, now, &args.keys, out).await
        }
        Command::Mark { key, state, note } => {
            cmd_mark(&paths, &key, &state, note.as_deref(), now, out)
        }
        Command::Record(args) => cmd_record(&paths, args, now, out),
        Command::Inbox { command } => match command {
            InboxCommand::Import { file } => cmd_inbox_import(&paths, &file, now, out),
        },
        Command::Today => cmd_today(&paths, now, out),
        Command::Task { command } => match command {
            TaskCommand::Add(args) => cmd_task_add(&paths, args, now, out),
            TaskCommand::Done { id } => cmd_task_done(&paths, id, out),
        },
        Command::Freelance { command } => {
            let ignore_window = match &command {
                FreelanceCommand::Profile(a) => a.window.ignore_window,
                FreelanceCommand::Proposal(a) => a.window.ignore_window,
            };
            let _run = match start_run(&paths, now, ignore_window, out)? {
                Ok(run) => run,
                Err(code) => return Ok(code),
            };
            cmd_freelance(&paths, &deps, now, command, out).await
        }
        Command::Approve { keys } => cmd_approve(&paths, &keys, out),
        Command::Resolve { keys } => cmd_resolve(&paths, &keys, now, out),
        Command::Confirm { keys } => cmd_confirm(&paths, &keys, now, out),
        Command::Digest => cmd_digest(&paths, now, out),
        Command::Top { n } => cmd_top(&paths, n, out),
        Command::Status => cmd_status(&paths, now, out),
        Command::Results { last } => cmd_results(&paths, last, out),
        Command::AutoApprove { value } => cmd_auto_approve(&paths, value.as_deref(), out),
        Command::Stop => cmd_stop(&paths, out),
        Command::Resume => cmd_resume(&paths, out),
        Command::InstallBrowsers => cmd_install_browsers(out).await,
        Command::Prep { command } => cmd_prep(&paths, command, now, out),
        Command::Panel(args) => cmd_panel(&paths, now, args, out).await,
    }
}

const STOP_MESSAGE: &str = "STOP activo: no se ejecuta nada. Usa `jobbot resume` para reanudar.";

/// The two switches in front of every mutating command. STOP is checked first and nothing
/// overrides it; the execution window comes second and `--ignore-window` skips only that.
/// Returns the exit code to stop with, or `None` when the command may go on.
fn blocked(
    paths: &Paths,
    now: DateTime<Utc>,
    ignore_window: bool,
    out: &mut dyn Write,
) -> Result<Option<i32>> {
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok(Some(EXIT_STOP));
    }
    if !ignore_window {
        // A missing or broken schedule is an error (exit 1), not a free pass: fail closed.
        let schedule = load_schedule(&paths.profile_dir.join("schedule.toml"))?;
        if !in_run_window(&schedule, now) {
            let next = next_window_start(&schedule, now).with_timezone(&Santiago);
            writeln!(
                out,
                "Fuera de la ventana de ejecución (horario laboral de Gonza): no se ejecuta nada. \
                 Próxima ventana: {} (hora de Santiago). Usa --ignore-window para forzar.",
                next.format("%Y-%m-%d %H:%M")
            )?;
            return Ok(Some(EXIT_OUTSIDE_WINDOW));
        }
    }
    Ok(None)
}

const LOCK_MESSAGE: &str = "Otra ejecución de jobbot está en curso (data/jobbot.lock): no se ejecuta nada. Espera a que termine.";

/// Takes `data/jobbot.lock` for as long as the returned file lives. The lock belongs to the open
/// file, so the system drops it if the program dies and it never goes stale. `None` means another
/// jobbot process holds it.
fn run_lock(paths: &Paths) -> Result<Option<std::fs::File>> {
    std::fs::create_dir_all(&paths.data_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.data_dir.join("jobbot.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// Watches the execution window while a command runs. `blocked` only looks at the moment the
/// command starts; a run that begins in the evening can last into Gonza's working hours, so the
/// long steps ask the watch again. The time is the start time plus the time elapsed since, which
/// keeps tests deterministic. A run started with `--ignore-window` never closes.
struct WindowWatch {
    schedule: Option<Schedule>,
    start: DateTime<Utc>,
    clock: std::time::Instant,
}

impl WindowWatch {
    /// The start time plus the time elapsed since: "now" for a run whose clock is injected.
    fn now(&self) -> DateTime<Utc> {
        let elapsed =
            TimeDelta::from_std(self.clock.elapsed()).unwrap_or_else(|_| TimeDelta::zero());
        self.start + elapsed
    }

    fn is_open(&self) -> bool {
        let Some(schedule) = &self.schedule else {
            return true;
        };
        in_run_window(schedule, self.now())
    }
}

/// What a mutating command holds while it runs: the run lock and the window watch.
struct RunGuard {
    _lock: std::fs::File,
    watch: WindowWatch,
}

/// The checks in front of every mutating command, in order: STOP, the execution window, then the
/// run lock (two jobbot processes writing the same database and driving the same browser profile
/// could send one form twice). `Err(code)` is the exit code to stop with.
fn start_run(
    paths: &Paths,
    now: DateTime<Utc>,
    ignore_window: bool,
    out: &mut dyn Write,
) -> Result<std::result::Result<RunGuard, i32>> {
    if let Some(code) = blocked(paths, now, ignore_window, out)? {
        return Ok(Err(code));
    }
    let Some(lock) = run_lock(paths)? else {
        writeln!(out, "{LOCK_MESSAGE}")?;
        return Ok(Err(EXIT_ERROR));
    };
    let schedule = if ignore_window {
        None
    } else {
        Some(load_schedule(&paths.profile_dir.join("schedule.toml"))?)
    };
    Ok(Ok(RunGuard {
        _lock: lock,
        watch: WindowWatch {
            schedule,
            start: now,
            clock: std::time::Instant::now(),
        },
    }))
}

const WINDOW_CLOSED_MESSAGE: &str =
    "La ventana de ejecución se cerró (empieza el horario laboral de Gonza)";

fn open_tracker(paths: &Paths) -> Result<Tracker> {
    std::fs::create_dir_all(&paths.data_dir)?;
    Tracker::open(&paths.data_dir.join("jobbot.db"))
}

/// The injected runner (tests) or the real `claude.exe`, started in the empty `data/claude-cwd`.
fn claude_runner(
    paths: &Paths,
    deps: &Deps,
    config: &ClaudeConfig,
) -> Result<Arc<dyn ClaudeRunner>> {
    if let Some(injected) = &deps.claude {
        return Ok(injected.clone());
    }
    let cli = CliClaude::locate(config.clone(), paths.data_dir.join("claude-cwd"))?;
    Ok(Arc::new(cli))
}

fn open_prep_store(paths: &Paths) -> Result<PrepStore> {
    std::fs::create_dir_all(&paths.data_dir)?;
    PrepStore::open(&paths.data_dir.join("prep.db"))
}

async fn cmd_claude_check(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    if deps.claude.is_none() {
        // Only the real program has a path and a version to report.
        let real = CliClaude::locate(config.clone(), paths.data_dir.join("claude-cwd"))?;
        writeln!(out, "Programa: {}", real.program().display())?;
        match real.version().await {
            Some(v) if v == TESTED_CLAUDE_VERSION => writeln!(out, "Versión: {v} (la probada)")?,
            Some(v) => writeln!(
                out,
                "Aviso: versión {v}, la probada es {TESTED_CLAUDE_VERSION}. Si algo falla, \
                 revisa que las banderas de claude -p sigan siendo las mismas."
            )?,
            None => writeln!(out, "Aviso: no pude leer la versión de claude.")?,
        }
    }
    let gate = ClaudeGate::new(runner.as_ref(), &tracker, &config);
    let seen = gate.check_account(now).await?;
    writeln!(out, "Cuenta: {}", seen.describe())?;
    print_account_warnings(&seen, out)?;
    if let Some(note) = unanchored_note(&config) {
        writeln!(out, "{note}")?;
    }
    let request = ClaudeRequest {
        stage: "check".to_string(),
        system: "You are a connectivity check. Answer in one word.".to_string(),
        prompt: "Reply with exactly the word OK and nothing else.".to_string(),
        schema: None,
    };
    let reply = gate.ask(&request, now).await?;
    writeln!(
        out,
        "Respuesta: {} · costo US${:.4}",
        reply.text.trim(),
        reply.cost_usd
    )?;
    let spent = tracker.claude_spend_since(now - TimeDelta::hours(24))?;
    writeln!(
        out,
        "Gastado en 24 h: US${spent:.2} de US${:.2}",
        config.daily_budget_usd
    )?;
    Ok(EXIT_OK)
}

/// The line that says, in red-flag words, that nothing fixes which account is the right one:
/// `None` when `expected_email` and `expected_org_id` are both set.
fn unanchored_note(config: &ClaudeConfig) -> Option<String> {
    (!config.is_anchored()).then(|| {
        "Aviso: cuenta sin anclar. profile/claude.local.toml no fija expected_email y \
         expected_org_id, así que cualquier sesión Pro o Max en la carpeta de jobbot pasa la \
         verificación, sea de quien sea. Fíjalos (jobbot claude-login te dice cómo)."
            .to_string()
    })
}

/// One `Aviso:` line per warning the preflight attached to the account (a managed policy found on
/// this PC). They never stop a command: Gonza reads them, the verdict stays what it was.
fn print_account_warnings(seen: &crate::claude::AccountSeen, out: &mut dyn Write) -> Result<()> {
    for warning in &seen.warnings {
        writeln!(out, "Aviso: {warning}")?;
    }
    Ok(())
}

/// One-time setup: `claude auth login` into this program's own config folder, then the same
/// preflight every run makes, so Gonza sees at once whether the login is the private one. With
/// an injected runner (tests) only the preflight runs: nothing else can open a browser.
async fn cmd_claude_login(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    if deps.claude.is_none() {
        let real = CliClaude::locate(config.clone(), paths.data_dir.join("claude-cwd"))?;
        writeln!(out, "{}", login_notice(&config, real.config_dir()))?;
        real.login().await?;
    }
    let gate = ClaudeGate::new(runner.as_ref(), &tracker, &config);
    match gate.check_account(now).await {
        Ok(seen) => {
            writeln!(out, "Sesión verificada. Cuenta: {}", seen.describe())?;
            print_account_warnings(&seen, out)?;
            if !config.is_anchored() {
                // Not in claude.toml: that file is committed and the repository may be published.
                writeln!(
                    out,
                    "Aviso: cuenta sin anclar. Sugerencia: para que cada corrida exija exactamente esta cuenta, crea \
                     profile/claude.local.toml (git lo ignora; no pongas el correo en \
                     claude.toml, que se publica con el repositorio) con:\n  \
                     expected_email = \"tu correo personal\"\n  expected_org_id = \"{}\"",
                    if seen.org_id.is_empty() {
                        "el orgId de `claude auth status`"
                    } else {
                        &seen.org_id
                    }
                )?;
            }
            Ok(EXIT_OK)
        }
        Err(err) => {
            writeln!(out, "La sesión no sirve: {err:#}")?;
            if is_account_error(&err) {
                // A login that cannot be used must not stay on this PC: close it, with the same
                // clean environment and folder that opened it.
                match runner.logout().await {
                    Ok(()) => writeln!(
                        out,
                        "Cerré esa sesión (claude auth logout) para no dejar guardadas las \
                         credenciales de una cuenta que jobbot no puede usar. Repite \
                         jobbot claude-login en una ventana privada del navegador."
                    )?,
                    Err(e) => writeln!(
                        out,
                        "No pude cerrar esa sesión ({e}). Ciérrala a mano antes de seguir: borra \
                         la carpeta de la sesión de jobbot (la que dice más arriba; por defecto \
                         %LOCALAPPDATA%\\jobbot\\claude-config)."
                    )?,
                }
            }
            Ok(EXIT_ERROR)
        }
    }
}

/// What `claude-login` says before the browser opens: whose account to choose, and that the
/// browser must not be one where the session of an employer is open (the sign-in page would offer
/// it, and one careless click would authorize Claude Code for that organization from this PC).
/// The address is shown masked: it is only there so Gonza knows which account to pick.
fn login_notice(config: &ClaudeConfig, config_dir: &Path) -> String {
    let which = match config.expected_email.trim() {
        "" => "Elige tu cuenta personal (y tu organización personal, si te pregunta).".to_string(),
        email => format!(
            "Elige la cuenta {} (y tu organización personal, si te pregunta).",
            crate::claude::mask_email(email)
        ),
    };
    format!(
        "Iniciando sesión en el directorio propio de jobbot: {}\n\
         ATENCIÓN: abre el enlace en una VENTANA PRIVADA del navegador (o en un navegador donde \
         NO esté abierta la sesión de claude.ai de una empresa ni de un cliente). Si se abre \
         sola con esa sesión, ciérrala y copia el enlace a la ventana privada.\n{which}",
        config_dir.display()
    )
}

fn format_report(r: &DiscoveryReport) -> String {
    let mut text = format!(
        "Descubrimiento: {} avisos · {} en cola · {} bajo el piso (excepción) · {} bajo el umbral · \
         {} excluidos · {} fuera de región · {} vencidos · {} ya vistos · {} actualizados · \
         podados: {} vencidos, {} fuera de región",
        r.fetched,
        r.queued,
        r.below_floor_flagged,
        r.below_threshold,
        r.excluded,
        r.region_skipped,
        r.stale,
        r.already_seen,
        r.refreshed,
        r.pruned_stale,
        r.pruned_region
    );
    for error in &r.errors {
        text.push_str(&format!("\n  ERROR {error}"));
    }
    text
}

async fn cmd_discover(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    rescore_first: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let tracker = open_tracker(paths)?;
    if rescore_first {
        // After a change to criteria.toml: stored jobs without an application get today's score.
        let r = rescore(&criteria, &tracker, now)?;
        writeln!(
            out,
            "Repuntuación: {} avisos revisados · {} nuevos en cola · {} bajo el piso (excepción)",
            r.rescored, r.newly_queued, r.newly_flagged
        )?;
    }
    // Injected sources (tests) or the real ones built from profile/sources.toml.
    let built;
    let sources: &[Box<dyn Source>] = match &deps.sources {
        Some(injected) => injected,
        None => {
            let config = load_sources_config(&paths.profile_dir.join("sources.toml"))?;
            let client = reqwest::Client::builder()
                .user_agent("jobbot/0.1")
                .build()?;
            built = build_sources(&config, &client);
            &built
        }
    };
    let report = discover(sources, &criteria, &exclusions, &tracker, now).await?;
    writeln!(out, "{}", format_report(&report))?;
    // run-daily reads this to skip discovery while the last one is recent. A run where every
    // source failed fetched nothing and is not "done": the next hourly run must try again.
    if report.fetched > 0 || report.errors.is_empty() {
        tracker.set_meta("last_discover_at", &now.to_rfc3339())?;
    }
    Ok(EXIT_OK)
}

async fn cmd_prepare(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let cv_dir = paths.data_dir.join("cv");
    match prepare_report(
        &tracker,
        &paths.cv_master,
        &cv_dir,
        deps.renderer.as_ref(),
        now,
    )
    .await
    {
        Ok(report) => {
            writeln!(out, "{} postulaciones preparadas", report.prepared.len())?;
            for skip in &report.skipped {
                writeln!(out, "{}", skip.line())?;
            }
            Ok(EXIT_OK)
        }
        Err(CvError::NotReady) => {
            writeln!(
                out,
                "CV no listo: cv/cv-master.md todavía tiene marcas ⟦ ⟧ sin completar."
            )?;
            Ok(EXIT_CV_NOT_READY)
        }
        Err(other) => Err(other.into()),
    }
}

/// Whether `submit_all` would call the filler at least once, so Chromium is started only when
/// there is a form to fill. The gate may still refuse every row (cooldowns, caps); that is fine.
fn has_work(prepared: &[ApplicationRow], mode: SubmitMode, in_review_phase: bool) -> bool {
    match mode {
        SubmitMode::Assist => !prepared.is_empty(),
        SubmitMode::DryRun => prepared.iter().any(|r| r.screenshot_path.is_none()),
        SubmitMode::Live => prepared
            .iter()
            .any(|r| r.approved || !in_review_phase || r.screenshot_path.is_none()),
    }
}

/// Which browsers a `submit` run has work for: the main one (persistent profile, fills and sends
/// the ordinary forms) and the inspector (headless, opens the handoff forms Gonza sends himself).
struct SubmitNeeds {
    filler: bool,
    inspector: bool,
}

/// Sorts the prepared applications into handoff forms (`handoff::is_handoff_job`) and ordinary
/// ones, so each browser is started only when there is a form for it.
fn submit_needs(
    tracker: &Tracker,
    prepared: &[ApplicationRow],
    mode: SubmitMode,
    in_review_phase: bool,
) -> Result<SubmitNeeds> {
    let mut ordinary = Vec::new();
    let mut inspector = false;
    for row in prepared {
        let handoff = tracker
            .get_job(&row.job_key)?
            .is_some_and(|job| is_handoff_job(&job));
        if handoff {
            inspector = true;
        } else {
            ordinary.push(row.clone());
        }
    }
    Ok(SubmitNeeds {
        filler: has_work(&ordinary, mode, in_review_phase),
        inspector,
    })
}

/// The filler of a run that has handoff forms only: they go through the inspector, so this is
/// never asked to open anything.
struct NoBrowser;

#[async_trait::async_trait]
impl FormFiller for NoBrowser {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &crate::answers::AnswerBank,
        _mode: SubmitMode,
        _shots_dir: &Path,
    ) -> Result<SubmitResult> {
        anyhow::bail!(
            "el navegador de envío no se inició (solo había formularios de handoff) y se pidió abrir {}",
            job.key()
        )
    }
}

async fn cmd_submit(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    headed: bool,
    mode: SubmitMode,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let bank = load_answers(&paths.profile_dir.join("answers.toml"))?;
    let tracker = open_tracker(paths)?;
    let prepared = tracker.list_by_state(State::Prepared)?;
    let in_review_phase =
        tracker.approved_submitted_total()? < i64::from(criteria.first_n_reviewed);
    let needs = submit_needs(&tracker, &prepared, mode, in_review_phase)?;
    if !needs.filler && !needs.inspector {
        // Nothing the filler would touch: do not start a browser just to find that out.
        writeln!(out, "Nada que enviar.")?;
        return Ok(EXIT_OK);
    }
    let launched;
    let filler: &dyn FormFiller = match &deps.filler {
        Some(injected) => injected.as_ref(),
        None if needs.filler => {
            launched =
                BrowserFiller::launch(&paths.data_dir.join("browser-profile"), headed).await?;
            &launched
        }
        None => &NoBrowser,
    };
    let launched_inspector;
    let inspector: Option<&dyn FormInspector> = match (&deps.inspector, &deps.filler) {
        (Some(injected), _) => Some(injected.as_ref()),
        // An injected filler stands for the whole browser side of a test: no real Chromium.
        (None, Some(_)) => None,
        (None, None) if needs.inspector => {
            launched_inspector = BrowserInspector::launch().await?;
            Some(&launched_inspector)
        }
        (None, None) => None,
    };
    let network_checker;
    let open_checker: Option<&dyn OpenChecker> = match (&deps.open_checker, &deps.filler) {
        (Some(injected), _) => Some(injected.as_ref()),
        // An injected filler stands for the whole outside world of a test: no network either.
        (None, Some(_)) => None,
        (None, None) => {
            network_checker = NetworkOpenChecker::new(
                reqwest::Client::builder()
                    .user_agent("jobbot/0.1")
                    .build()?,
            );
            Some(&network_checker)
        }
    };
    match mode {
        SubmitMode::DryRun => writeln!(
            out,
            "Modo prueba en seco: se llena el formulario y se guarda una captura, no se envía. Si faltan respuestas, se llena el resto y la captura marca en rojo lo que falta. Usa --live para enviar o --assist para completar y enviar tú."
        )?,
        SubmitMode::Assist => writeln!(
            out,
            "Modo asistido: se llena cada formulario en el navegador; lo que no se sabe responder queda marcado en rojo para que lo completes tú, y el clic en enviar también lo das tú (hasta 10 minutos por formulario; cierra la pestaña para saltarte uno). Lo que envíes queda aprobado."
        )?,
        SubmitMode::Live => {}
    }
    let outcomes = submit_all_while(
        &tracker,
        &criteria,
        &exclusions,
        &bank,
        filler,
        inspector,
        open_checker,
        mode,
        &paths.data_dir,
        &paths.stop_file,
        now,
        &|| watch.is_open(),
    )
    .await?;
    for o in &outcomes {
        writeln!(
            out,
            "{:12} {} {}",
            o.status,
            o.job_key,
            o.reason.as_deref().unwrap_or("")
        )?;
    }
    if outcomes
        .iter()
        .any(|o| o.reason.as_deref().is_some_and(is_handoff_reason))
    {
        writeln!(
            out,
            "Handoff: esos formularios los envías tú desde tu navegador normal, copiando de la hoja de respuestas; después `jobbot confirm KEY` los marca como enviados."
        )?;
    }
    if outcomes.iter().any(|o| {
        o.status == "needs_gonza"
            && o.reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("faltan respuestas"))
    }) {
        writeln!(
            out,
            "Para responder tú lo que falta: `jobbot resolve KEY` devuelve la postulación a la cola y `jobbot submit --assist` la llena en un navegador visible y te deja lo demás."
        )?;
    }
    if !watch.is_open() {
        writeln!(
            out,
            "{WINDOW_CLOSED_MESSAGE}: el envío se detuvo y lo que falta sigue en la cola."
        )?;
    } else if outcomes.is_empty() {
        writeln!(out, "Nada que enviar.")?;
    }
    Ok(EXIT_OK)
}

fn cmd_approve(paths: &Paths, keys: &[String], out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    for key in keys {
        writeln!(out, "{}", actions::approve(&tracker, key)?.message)?;
    }
    Ok(EXIT_OK)
}

fn cmd_resolve(
    paths: &Paths,
    keys: &[String],
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    for key in keys {
        writeln!(out, "{}", actions::resolve(&tracker, key, now)?.message)?;
    }
    Ok(EXIT_OK)
}

/// For `unconfirmed_submit` and assisted forms Gonza finished himself.
fn cmd_confirm(
    paths: &Paths,
    keys: &[String],
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    for key in keys {
        writeln!(out, "{}", actions::confirm(&tracker, key, now)?.message)?;
    }
    Ok(EXIT_OK)
}

async fn cmd_dossier(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    keys: &[String],
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    let store = open_prep_store(paths)?;
    let cv_md = std::fs::read_to_string(&paths.cv_master)
        .with_context(|| format!("leyendo {}", paths.cv_master.display()))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let gate = ClaudeGate::new(runner.as_ref(), &tracker, &config);
    // Before any prompt: a wrong login stops the whole command, not one dossier.
    gate.check_account(now).await?;
    let dir = paths.data_dir.join("dossiers");
    let mut code = EXIT_OK;
    for key in keys {
        let Some(job) = tracker.get_job(key)? else {
            writeln!(out, "Aviso desconocido: {key}")?;
            code = EXIT_ERROR;
            continue;
        };
        match build_dossier(&gate, &store, &job, &cv_md, &exclusions, &dir, now).await {
            Ok(outcome) => writeln!(out, "{}", outcome.describe())?,
            Err(err) => {
                writeln!(out, "Error en {key}: {err:#}")?;
                code = EXIT_ERROR;
                if is_budget_error(&err) {
                    break; // every further call would be refused the same way
                }
            }
        }
    }
    Ok(code)
}

async fn cmd_freelance(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    command: FreelanceCommand,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let freelance = load_freelance_config(&paths.profile_dir.join("freelance.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let platform = match &command {
        FreelanceCommand::Profile(a) => a.platform.clone(),
        FreelanceCommand::Proposal(a) => a.platform.clone(),
    };
    // Say what is missing before reading anything else or touching Claude.
    if let Err(reason) = freelance.check_ready(&platform) {
        writeln!(out, "{reason}")?;
        return Ok(EXIT_ERROR);
    }
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    let gate = ClaudeGate::new(runner.as_ref(), &tracker, &config);
    gate.check_account(now).await?;
    let cv_md = std::fs::read_to_string(&paths.cv_master)
        .with_context(|| format!("leyendo {}", paths.cv_master.display()))?;
    let dir = paths.data_dir.join("freelance");
    let path = match command {
        FreelanceCommand::Profile(a) => {
            draft_profile(
                &gate,
                &freelance,
                &exclusions,
                &cv_md,
                &a.platform,
                &dir,
                now,
            )
            .await?
        }
        FreelanceCommand::Proposal(a) => {
            let posting = std::fs::read_to_string(&a.job_file)
                .with_context(|| format!("leyendo {}", a.job_file.display()))?;
            let name = a
                .job_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            draft_proposal(
                &gate,
                &freelance,
                &exclusions,
                &cv_md,
                &a.platform,
                &posting,
                &name,
                &dir,
                now,
            )
            .await?
        }
    };
    writeln!(out, "Borrador listo: {}", path.display())?;
    writeln!(
        out,
        "Es un borrador: revísalo y publícalo tú; el programa no envía nada a ninguna plataforma."
    )?;
    Ok(EXIT_OK)
}

/// Records a company's answer until Gmail does it (plan 1b-3).
fn cmd_mark(
    paths: &Paths,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = actions::mark(&tracker, key, state_text, note, now)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Records an application that went out outside jobbot. Like `mark` and `confirm` it writes only
/// the tracker, so it ignores STOP and the execution window: refusing to write down a form that
/// was already sent would only leave the tracker wrong.
fn cmd_record(
    paths: &Paths,
    args: RecordArgs,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let at = match args.at.as_deref() {
        None => now,
        Some(text) => match DateTime::parse_from_rfc3339(text.trim()) {
            Ok(time) => time.with_timezone(&Utc),
            Err(_) => {
                writeln!(
                    out,
                    "--at debe ser una fecha RFC 3339, por ejemplo 2026-10-05T05:21:42Z (recibí «{text}»)"
                )?;
                return Ok(EXIT_ERROR);
            }
        },
    };
    if at > now + TimeDelta::minutes(5) {
        writeln!(
            out,
            "--at está en el futuro ({}): una postulación no se registra antes de enviarse.",
            at.to_rfc3339()
        )?;
        return Ok(EXIT_ERROR);
    }
    let spec = actions::RecordSpec {
        key: args.key,
        company: args.company.unwrap_or_default(),
        title: args.title.unwrap_or_default(),
        url: args.url.unwrap_or_default(),
        location: args.location.unwrap_or_default(),
        cv_path: args.cv,
        salary_min_monthly_usd: args.salary_min,
        salary_max_monthly_usd: args.salary_max,
        note: args.note,
    };
    let tracker = open_tracker(paths)?;
    let outcome = actions::record(&tracker, &spec, at)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Imports the emails that Claude read in Gmail and wrote to `file`. Like `mark` and `record` it
/// writes only the tracker, so it ignores STOP and the execution window: nothing is sent anywhere.
/// A rejected message makes the exit code 1, but the valid ones were imported all the same.
fn cmd_inbox_import(
    paths: &Paths,
    file: &Path,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    // A file that is too big is refused before it is read.
    let size = std::fs::metadata(file)
        .with_context(|| format!("leyendo {}", file.display()))?
        .len();
    inbox::check_size(size).with_context(|| file.display().to_string())?;
    let text =
        std::fs::read_to_string(file).with_context(|| format!("leyendo {}", file.display()))?;
    let items = inbox::parse_items(&text).with_context(|| file.display().to_string())?;
    let tracker = open_tracker(paths)?;
    let report = inbox::import(&tracker, &items, now)?;
    for line in &report.lines {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "{}", report.summary())?;
    Ok(if report.rejected > 0 {
        EXIT_ERROR
    } else {
        EXIT_OK
    })
}

/// Read-only, like `top` and `status`: it works under STOP and outside the window.
fn cmd_today(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    write!(out, "{}", build_today(&tracker, now)?)?;
    Ok(EXIT_OK)
}

/// Adds a task to Gonza's list. Writes only the tracker: it ignores STOP and the window.
fn cmd_task_add(
    paths: &Paths,
    args: TaskAddArgs,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let due_at = match args.due.as_deref() {
        None => None,
        Some(text) => match DateTime::parse_from_rfc3339(text.trim()) {
            Ok(time) => Some(time.with_timezone(&Utc)),
            Err(_) => {
                writeln!(
                    out,
                    "--due debe ser una fecha RFC 3339, por ejemplo 2026-10-08T15:00:00Z (recibí «{}»)",
                    outbound::plain_line(text)
                        .chars()
                        .take(40)
                        .collect::<String>()
                )?;
                return Ok(EXIT_ERROR);
            }
        },
    };
    let spec = actions::TaskSpec {
        text: args.text,
        due_at,
        job_key: args.job_key,
        link: args.link,
    };
    let tracker = open_tracker(paths)?;
    let outcome = actions::task_add(&tracker, &spec, now)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Marks one of Gonza's tasks as done. Writes only the tracker: it ignores STOP and the window.
fn cmd_task_done(paths: &Paths, id: i64, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = actions::task_done(&tracker, id)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// The last step of `run-daily`: the dossier of every application that has an answer (an
/// interview first, then a test, then a reply) and has none yet, at most `max_dossiers_per_run`.
/// It looks at STOP and at the window before every call, and stops at the first failure: the next
/// hourly run tries again, and a Claude problem must not cost the run its digest.
async fn run_dossier_step(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<usize> {
    let tracker = open_tracker(paths)?;
    let dir = paths.data_dir.join("dossiers");
    let mut due: Vec<String> = Vec::new();
    for state in [State::Interview, State::TechnicalTest, State::Replied] {
        for row in tracker.list_by_state(state)? {
            if !dossier_path(&dir, &row.job_key).exists() {
                due.push(row.job_key);
            }
        }
    }
    if due.is_empty() {
        return Ok(0); // nothing to do: not even claude.toml or claude.exe is looked at
    }
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    due.truncate(config.max_dossiers_per_run);
    let cv_md = std::fs::read_to_string(&paths.cv_master)
        .with_context(|| format!("leyendo {}", paths.cv_master.display()))?;
    let runner = claude_runner(paths, deps, &config)?;
    let store = open_prep_store(paths)?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let gate = ClaudeGate::new(runner.as_ref(), &tracker, &config);
    // A wrong login is an account error: `run_daily_steps` turns it into exit 1.
    let seen = gate.check_account(now).await?;
    print_account_warnings(&seen, out)?;
    let mut written = 0;
    for key in due {
        if paths.stop_file.exists() || !watch.is_open() {
            writeln!(
                out,
                "Dossiers: se detiene (archivo STOP o fin de la ventana)."
            )?;
            break;
        }
        let Some(job) = tracker.get_job(&key)? else {
            continue;
        };
        match build_dossier(&gate, &store, &job, &cv_md, &exclusions, &dir, now).await {
            Ok(outcome) => {
                writeln!(out, "{}", outcome.describe())?;
                written += 1;
            }
            Err(err) => {
                writeln!(out, "Dossier de {key}: {err:#}")?;
                break;
            }
        }
    }
    Ok(written)
}

fn cmd_digest(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let text = build_digest(&tracker, now, 5)?;
    // Named after Gonza's day, like the title inside it.
    let file = paths.data_dir.join(format!(
        "digest-{}.md",
        now.with_timezone(&Santiago).format("%Y-%m-%d")
    ));
    std::fs::write(&file, &text)?;
    writeln!(out, "{text}")?;
    Ok(EXIT_OK)
}

fn cmd_top(paths: &Paths, n: usize, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    for r in tracker.top_scored(n)? {
        writeln!(
            out,
            "{:5.1}  {} — {}  [{}] {}",
            r.score,
            r.company,
            r.title,
            r.state.as_str(),
            r.url
        )?;
    }
    // The forms the bot filled and left for Gonza's own send are not in the queue above anymore.
    let handoff: Vec<ApplicationRow> = tracker
        .list_by_state(State::NeedsGonza)?
        .into_iter()
        .filter(|r| r.needs_reason.as_deref().is_some_and(is_handoff_reason))
        .collect();
    if !handoff.is_empty() {
        writeln!(out, "\nPara enviar tú desde tu navegador normal (handoff):")?;
        for r in &handoff {
            writeln!(
                out,
                "{:5.1}  {} — {}  [{}]\n       {}\n       cuando lo envíes: jobbot confirm {}",
                r.score,
                r.company,
                r.title,
                r.state.as_str(),
                r.needs_reason.as_deref().unwrap_or(""),
                r.job_key
            )?;
        }
    }
    Ok(EXIT_OK)
}

/// Read-only: the applications per state, then the three things that say whether the hourly task
/// is doing its job (when `run-daily` last ended and how, which Claude login was last verified,
/// whether STOP is on). The last run is only what ran: a task turned away by the window or STOP
/// leaves it alone, so an evening `status` still shows the last real run.
fn cmd_status(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let counts = tracker.counts()?;
    if counts.is_empty() {
        writeln!(out, "Estado: sin postulaciones todavía.")?;
    }
    for (state, n) in counts {
        writeln!(out, "{state:12} {n}")?;
    }
    writeln!(out)?;
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
    }
    writeln!(out, "{}", describe_last_run(&tracker, now)?)?;
    writeln!(out, "{}", describe_last_account(&tracker, now)?)?;
    writeln!(out, "{}", describe_auto_approve(&tracker)?)?;
    // Without the keys that pin the account, "verificada" only means "a Pro or Max login": say it.
    let anchor = load_claude_config(&paths.profile_dir.join("claude.toml"));
    if let Some(note) = unanchored_note(&anchor.unwrap_or_default()) {
        writeln!(out, "{note}")?;
    }
    Ok(EXIT_OK)
}

/// A stamp for Gonza: his clock (Santiago) and how long ago, e.g. `2026-10-14 19:00 (hace 2 h)`;
/// `name_the_zone` makes it `(hora de Santiago, hace 2 h)`.
fn stamp_with_age(at: DateTime<Utc>, now: DateTime<Utc>, name_the_zone: bool) -> String {
    let age = now - at;
    let ago = if age < TimeDelta::minutes(1) {
        "hace un momento".to_string()
    } else if age < TimeDelta::hours(1) {
        format!("hace {} min", age.num_minutes())
    } else if age < TimeDelta::days(2) {
        format!("hace {} h", age.num_hours())
    } else {
        format!("hace {} días", age.num_days())
    };
    let zone = if name_the_zone {
        "hora de Santiago, "
    } else {
        ""
    };
    format!(
        "{} ({zone}{ago})",
        at.with_timezone(&Santiago).format("%Y-%m-%d %H:%M")
    )
}

/// The `Última corrida` lines of `status`, from `meta.last_run_*`.
fn describe_last_run(tracker: &Tracker, now: DateTime<Utc>) -> Result<String> {
    let at = tracker
        .get_meta("last_run_at")?
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|time| time.with_timezone(&Utc));
    let Some(at) = at else {
        return Ok("Última corrida: ninguna todavía (la tarea programada la anota al terminar cada run-daily).".to_string());
    };
    let result = tracker.get_meta("last_run_result")?.unwrap_or_default();
    let result = match result.as_str() {
        "ok" => "ok".to_string(),
        "stopped" => "se detuvo por STOP".to_string(),
        "window_closed" => "se cerró la ventana de ejecución".to_string(),
        other => other.to_string(),
    };
    let mut text = format!(
        "Última corrida: {}: {result}",
        stamp_with_age(at, now, true)
    );
    let steps = tracker
        .get_meta("last_run_steps")?
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok());
    if let Some(steps) = steps {
        text.push_str("\n  ");
        text.push_str(&describe_steps(&steps));
    }
    Ok(text)
}

/// `descubrir: 1 nuevas · preparar: 1 preparadas · ...` from the JSON of `meta.last_run_steps`.
fn describe_steps(steps: &serde_json::Value) -> String {
    let count = |step: &str, key: &str| steps[step][key].as_i64().unwrap_or(0);
    let mut parts = Vec::new();
    for (step, label) in RUN_STEPS {
        let entry = &steps[step];
        if entry.is_null() {
            continue;
        }
        let what = if entry["ran"].as_bool() != Some(true) {
            match entry["skipped"].as_str().unwrap_or_default() {
                "cooldown" => format!("omitido (hace menos de {DISCOVER_COOLDOWN_HOURS} h)"),
                "stop" => "omitido (STOP)".to_string(),
                "window_closed" => "omitido (ventana cerrada)".to_string(),
                "after_error" => "omitido (tras un error)".to_string(),
                "error" => "con error".to_string(),
                other => format!("omitido ({other})"),
            }
        } else {
            match step {
                "discover" => format!("{} nuevas", count(step, "new")),
                "prepare" => format!("{} preparadas", count(step, "prepared")),
                "submit" => format!(
                    "{} enviadas, {} para ti",
                    count(step, "submitted"),
                    count(step, "needs_gonza")
                ),
                _ => format!("{} escritos", count(step, "written")),
            }
        };
        parts.push(format!("{label}: {what}"));
    }
    parts.join(" · ")
}

/// The `Cuenta de Claude` line of `status`, from what the last preflight left in `meta`.
fn describe_last_account(tracker: &Tracker, now: DateTime<Utc>) -> Result<String> {
    let record = tracker
        .get_meta("claude_account_last")?
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok());
    let Some(record) = record else {
        return Ok(
            "Cuenta de Claude: sin verificar todavía (jobbot claude-login, o jobbot claude-check)."
                .to_string(),
        );
    };
    let when = record["checked_at"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| stamp_with_age(time.with_timezone(&Utc), now, false))
        .unwrap_or_else(|| "—".to_string());
    // What the check noticed without refusing (a managed policy on this PC), under either verdict.
    let warnings: String = record["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|warning| warning.as_str())
        .map(|warning| format!("\n  Aviso: {warning}"))
        .collect();
    if record["ok"].as_bool() != Some(true) {
        return Ok(format!(
            "Cuenta de Claude: la última verificación falló ({when}): {}{warnings}",
            record["message"].as_str().unwrap_or("sin detalle")
        ));
    }
    let field = |key: &str| match record[key].as_str() {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => "—".to_string(),
    };
    Ok(format!(
        "Cuenta de Claude: verificada {when} · método {} · proveedor {} · suscripción {} · correo {} · org {}{warnings}",
        field("auth_method"),
        field("api_provider"),
        field("subscription"),
        field("email"),
        field("org_id"),
    ))
}

/// Read-only, like `top` and `status`: it works under STOP and outside the window.
fn cmd_results(paths: &Paths, last: u64, out: &mut dyn Write) -> Result<i32> {
    let last = usize::try_from(last).unwrap_or(usize::MAX);
    let results = read_last(&results_path(&paths.data_dir), last)?;
    writeln!(out, "{}", format_table(&results))?;
    Ok(EXIT_OK)
}

/// The auto-approval threshold (see `actions::set_auto_approve_min`): shown with no argument, set
/// with a whole number, turned off with `off`. Like the panel's settings it only writes the tracker.
fn cmd_auto_approve(paths: &Paths, value: Option<&str>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = match value.map(str::trim) {
        None => {
            writeln!(out, "{}", describe_auto_approve(&tracker)?)?;
            return Ok(EXIT_OK);
        }
        Some(text) if text.eq_ignore_ascii_case("off") => {
            actions::set_auto_approve_min(&tracker, None, None)?
        }
        Some(text) => {
            let Ok(number) = text.parse::<i64>() else {
                writeln!(
                    out,
                    "El umbral debe ser un número entero o `off`, no «{}».",
                    crate::outbound::clean(text, 40)
                )?;
                return Ok(EXIT_ERROR);
            };
            let threshold = load_criteria(&paths.profile_dir.join("criteria.toml"))
                .ok()
                .map(|criteria| criteria.apply_threshold);
            actions::set_auto_approve_min(&tracker, Some(number), threshold)?
        }
    };
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// The `Auto-aprobación` line of `status` and of `jobbot auto-approve`.
fn describe_auto_approve(tracker: &Tracker) -> Result<String> {
    Ok(match actions::auto_approve_min(tracker)? {
        None => "Auto-aprobación: apagada.".to_string(),
        Some(min) => format!("Auto-aprobación: desde {min} puntos."),
    })
}

fn cmd_stop(paths: &Paths, out: &mut dyn Write) -> Result<i32> {
    actions::stop(paths)?;
    writeln!(
        out,
        "STOP creado: discover, prepare, submit, run-daily, dossier, freelance y claude-check quedan detenidos."
    )?;
    Ok(EXIT_OK)
}

fn cmd_resume(paths: &Paths, out: &mut dyn Write) -> Result<i32> {
    actions::resume(paths)?;
    writeln!(out, "STOP eliminado.")?;
    Ok(EXIT_OK)
}

async fn cmd_install_browsers(out: &mut dyn Write) -> Result<i32> {
    writeln!(out, "Instalando Chromium (una sola vez, unos 250 MB)...")?;
    playwright_rs::install_browsers(Some(&["chromium"])).await?;
    writeln!(out, "Chromium instalado.")?;
    Ok(EXIT_OK)
}

/// The scheduled job. The steps run in order and the first one that fails skips the rest, but
/// the digest is always written. Submission is `SubmitMode::Live`: the gate inside `submit_all`
/// still turns every application that Gonza has not approved yet into a dry run.
async fn cmd_run_daily(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<i32> {
    writeln!(out, "== run-daily {} ==", now.format("%Y-%m-%d %H:%M UTC"))?;
    let mut log = StepLog::default();
    // A step that fails outright (a broken profile file, a database error) is shown and costs the
    // other steps, but never the digest: it is what Gonza reads to know what happened.
    let (steps, end) = match run_daily_steps(paths, deps, now, watch, &mut log, out).await {
        Ok(done) => done,
        Err(err) => {
            writeln!(out, "Error en un paso de run-daily: {err:#}")?;
            (EXIT_ERROR, RunEnd::Failed(format!("{err:#}")))
        }
    };
    // The record is a convenience for `status` and the panel: failing to write it must not cost
    // the run its digest or its exit code.
    if let Err(err) = record_run(paths, watch.now(), &end, &mut log) {
        writeln!(out, "No pude anotar la corrida: {err:#}")?;
    }
    let digest = cmd_digest(paths, now, out)?;
    Ok(if steps != EXIT_OK { steps } else { digest })
}

/// The steps of `run-daily`, in order: the key in `meta.last_run_steps` and the word `status` uses.
const RUN_STEPS: [(&str, &str); 4] = [
    ("discover", "descubrir"),
    ("prepare", "preparar"),
    ("submit", "enviar"),
    ("dossiers", "dossiers"),
];

/// Longest error text kept in `meta.last_run_result`: one line the panel can show whole.
const RUN_ERROR_CHARS: usize = 300;

/// How a `run-daily` ended: the value of `meta.last_run_result`.
enum RunEnd {
    /// Every step that was due ran (a failure of the dossiers alone does not count).
    Done,
    /// A STOP file appeared between two steps.
    Stopped,
    /// The execution window closed between two steps.
    WindowClosed,
    /// A step failed; the text is its error, cut and on one line when written.
    Failed(String),
}

impl RunEnd {
    fn as_meta(&self) -> String {
        match self {
            RunEnd::Done => "ok".to_string(),
            RunEnd::Stopped => "stopped".to_string(),
            RunEnd::WindowClosed => "window_closed".to_string(),
            RunEnd::Failed(why) => format!(
                "error: {}",
                outbound::plain_line(why)
                    .chars()
                    .take(RUN_ERROR_CHARS)
                    .collect::<String>()
            ),
        }
    }

    /// Why a step that never started did not: the `skipped` of its entry in the log.
    fn skip_reason(&self) -> &'static str {
        match self {
            RunEnd::Done => "not_due",
            RunEnd::Stopped => "stop",
            RunEnd::WindowClosed => "window_closed",
            RunEnd::Failed(_) => "after_error",
        }
    }
}

/// What each step of one `run-daily` did, as the JSON of `meta.last_run_steps`.
#[derive(Default)]
struct StepLog(serde_json::Map<String, serde_json::Value>);

impl StepLog {
    fn ran(&mut self, step: &str, mut items: serde_json::Value) {
        items["ran"] = serde_json::Value::Bool(true);
        self.0.insert(step.to_string(), items);
    }

    fn skipped(&mut self, step: &str, why: &str) {
        self.0.insert(
            step.to_string(),
            serde_json::json!({"ran": false, "skipped": why}),
        );
    }

    /// The steps the run never reached say why, so the record always lists all four.
    fn close(&mut self, end: &RunEnd) {
        for (step, _) in RUN_STEPS {
            if !self.0.contains_key(step) {
                self.skipped(step, end.skip_reason());
            }
        }
    }
}

/// Writes `meta.last_run_at` (the end of the run), `last_run_result` and `last_run_steps`. Only a
/// run that got past STOP, the window and the lock gets here: the hourly task turned away at the
/// door writes nothing, so the last real run stays visible and `run-daily.cmd` keeps its log empty.
/// The stamp goes last, so whoever sees a new one finds the result that belongs to it.
fn record_run(
    paths: &Paths,
    finished: DateTime<Utc>,
    end: &RunEnd,
    log: &mut StepLog,
) -> Result<()> {
    log.close(end);
    let tracker = open_tracker(paths)?;
    tracker.set_meta("last_run_result", &end.as_meta())?;
    tracker.set_meta(
        "last_run_steps",
        &serde_json::Value::Object(log.0.clone()).to_string(),
    )?;
    tracker.set_meta("last_run_at", &finished.to_rfc3339())?;
    Ok(())
}

/// Transitions into `states` ever recorded; what a step moved is the difference across it.
fn transitions_into(paths: &Paths, states: &[State]) -> Result<i64> {
    open_tracker(paths)?.count_events(states, DateTime::UNIX_EPOCH)
}

/// When the window closes in the middle of a run (a slow board, a long queue of CVs) the rest of
/// the steps are skipped. That is not a failure and not exit 4: `run-daily.cmd` discards the log
/// of an exit 4, and this run did real work that belongs in it.
fn window_closed(watch: &WindowWatch, skipped: &str, out: &mut dyn Write) -> Result<bool> {
    if watch.is_open() {
        return Ok(false);
    }
    writeln!(out, "{WINDOW_CLOSED_MESSAGE}: se omite {skipped}.")?;
    Ok(true)
}

/// The steps in order. `log` collects what each did while it runs, so that a step that fails
/// outright (`Err`) still leaves the record of the ones before it. Returns the exit code and how
/// the run ended.
async fn run_daily_steps(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    log: &mut StepLog,
    out: &mut dyn Write,
) -> Result<(i32, RunEnd)> {
    // The task fires hourly; the boards do not change that fast. Open the tracker only to read
    // the stamp, so the discover step gets its own connection.
    let last_discover = open_tracker(paths)?
        .get_meta("last_discover_at")?
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|t| t.with_timezone(&Utc));
    match last_discover {
        Some(last) if now - last < TimeDelta::hours(DISCOVER_COOLDOWN_HOURS) => {
            writeln!(
                out,
                "Descubrimiento omitido: el último fue hace {} min (se repite cada {} h).",
                (now - last).num_minutes(),
                DISCOVER_COOLDOWN_HOURS
            )?;
            log.skipped("discover", "cooldown");
        }
        _ => {
            let before = transitions_into(paths, &[State::Scored])?;
            // Until the step answers it counts as failed: a step that errors out (`?`) leaves this.
            log.skipped("discover", "error");
            let code = cmd_discover(paths, deps, now, false, out).await?;
            let new = transitions_into(paths, &[State::Scored])? - before;
            log.ran("discover", serde_json::json!({"new": new}));
            if code != EXIT_OK {
                let why = format!("descubrir terminó con código {code}");
                return Ok((code, RunEnd::Failed(why)));
            }
        }
    }
    // Gonza may create STOP while a long run is in progress: look again between steps.
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok((EXIT_STOP, RunEnd::Stopped));
    }
    if window_closed(watch, "la preparación de CVs y el envío", out)? {
        return Ok((EXIT_OK, RunEnd::WindowClosed));
    }
    let before = transitions_into(paths, &[State::Prepared])?;
    log.skipped("prepare", "error");
    let code = cmd_prepare(paths, deps, now, out).await?;
    let prepared = transitions_into(paths, &[State::Prepared])? - before;
    if code != EXIT_OK {
        writeln!(out, "prepare terminó con código {code}; se omite el envío.")?;
        log.ran(
            "prepare",
            serde_json::json!({"prepared": prepared, "code": code}),
        );
        let why = format!("prepare terminó con código {code}");
        return Ok((code, RunEnd::Failed(why)));
    }
    log.ran("prepare", serde_json::json!({"prepared": prepared}));
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok((EXIT_STOP, RunEnd::Stopped));
    }
    if window_closed(watch, "el envío", out)? {
        return Ok((EXIT_OK, RunEnd::WindowClosed));
    }
    let sent = transitions_into(paths, &[State::Submitted])?;
    let handed = transitions_into(paths, &[State::NeedsGonza])?;
    log.skipped("submit", "error");
    let code = cmd_submit(paths, deps, now, false, SubmitMode::Live, watch, out).await?;
    log.ran(
        "submit",
        serde_json::json!({
            "submitted": transitions_into(paths, &[State::Submitted])? - sent,
            "needs_gonza": transitions_into(paths, &[State::NeedsGonza])? - handed,
        }),
    );
    // The dossiers come last: they need no browser and a problem with them changes nothing above.
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok((EXIT_STOP, RunEnd::Stopped));
    }
    if window_closed(watch, "los dossiers", out)? {
        return Ok((code, RunEnd::WindowClosed));
    }
    match run_dossier_step(paths, deps, now, watch, out).await {
        Ok(written) => log.ran("dossiers", serde_json::json!({"written": written})),
        // A missing program or a spent budget only costs the dossiers. A login that is not the
        // private subscription is different: nothing was asked, and the run fails so the
        // scheduled task's log and the digest show it before anything else is tried.
        Err(err) if is_account_error(&err) => {
            writeln!(out, "Claude no se usó: {err:#}")?;
            log.skipped("dossiers", "error");
            let why = format!("Claude no se usó: {err:#}");
            return Ok((EXIT_ERROR, RunEnd::Failed(why)));
        }
        Err(err) => {
            writeln!(out, "Dossiers omitidos: {err:#}")?;
            log.skipped("dossiers", "error");
        }
    }
    Ok((code, RunEnd::Done))
}

/// The interview drill. Claude runs the mock interview in the chat: `next` to pick a question,
/// `reveal` to compare Gonza's answer with the reference, `grade` to record the score.
/// Study commands ignore the STOP file and the execution window: they send nothing anywhere.
fn cmd_prep(
    paths: &Paths,
    command: PrepCommand,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let store = open_prep_store(paths)?;
    let questions = store.all_questions()?;
    match command {
        PrepCommand::Next { topic } => {
            match store.next_question(&questions, now, topic.as_deref())? {
                Some(q) => writeln!(out, "{} [{}]\n{}", q.id, q.topic, q.question)?,
                None => writeln!(out, "Estás al día: no hay preguntas pendientes.")?,
            }
        }
        PrepCommand::Reveal { id } => match questions.iter().find(|q| q.id == id) {
            Some(q) => writeln!(out, "{}", q.answer)?,
            None => {
                writeln!(out, "Pregunta desconocida: {id}")?;
                return Ok(EXIT_ERROR);
            }
        },
        PrepCommand::Grade { id, grade } => {
            let outcome = actions::grade_card(&store, &questions, &id, grade, now)?;
            writeln!(out, "{}", outcome.message)?;
            if !outcome.ok {
                return Ok(EXIT_ERROR);
            }
        }
        PrepCommand::Stats => {
            let stats = store.stats(&questions, now)?;
            writeln!(
                out,
                "Vistas: {}/{} · Pendientes hoy: {}",
                stats.seen, stats.total, stats.due
            )?;
            for (topic, average) in store.weak_topics(3, 10)? {
                writeln!(out, "Tema débil: {topic} (promedio {average:.1})")?;
            }
        }
    }
    Ok(EXIT_OK)
}

/// Serves the local panel until the process is stopped (Ctrl+C). Like `prep` and `approve`, it
/// ignores STOP and the window: it only reads and writes the tracker and the prep store, on this
/// PC, and starts no discovery, browser or Claude. The clock is the start time plus the time
/// elapsed since, so it follows `main`'s reading of the real one. The one thing it
/// asks of Windows is, read-only and in the background, whether the scheduled task is registered
/// and when it fires next (`panel::scheduler`).
async fn cmd_panel(
    paths: &Paths,
    now: DateTime<Utc>,
    args: PanelArgs,
    out: &mut dyn Write,
) -> Result<i32> {
    let started = std::time::Instant::now();
    let open_panel = || {
        let clock = Box::new(move || {
            now + TimeDelta::from_std(started.elapsed()).unwrap_or_else(|_| TimeDelta::zero())
        });
        Panel::open(paths.clone(), clock)
            .map(|panel| panel.with_task_query(WindowsTasks::default_task()))
    };
    let server = match args.port {
        Some(port) => PanelServer::start(open_panel()?, port)?,
        // A port that stays the same keeps the browser's saved settings (they belong to the origin, port included).
        None => match PanelServer::start(open_panel()?, PREFERRED_PANEL_PORT) {
            Ok(server) => server,
            Err(_) => PanelServer::start(open_panel()?, 0)?,
        },
    };
    let url = server.url();
    writeln!(out, "Panel en {url}")?;
    writeln!(
        out,
        "Solo este PC puede abrirlo. El enlace trae una clave que cambia cada vez que lo inicias. Ctrl+C lo cierra."
    )?;
    if !args.no_open
        && let Err(err) = open_in_browser(&url)
    {
        writeln!(
            out,
            "No pude abrir el navegador ({err}): copia el enlace de arriba."
        )?;
    }
    tokio::task::spawn_blocking(move || server.wait()).await?;
    writeln!(out, "El panel se cerró.")?;
    Ok(EXIT_ERROR)
}

/// `start` is a built-in of cmd.exe; the empty first argument is the window title it would
/// otherwise take from the address.
fn open_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use chrono::TimeZone;

    use super::*;
    use crate::answers::AnswerBank;
    use crate::claude::{ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner};
    use crate::form::{SubmitMode, SubmitResult, SubmitStatus};
    use crate::models::{FetchResult, Job};
    use crate::open_check::OpenStatus;
    use crate::testing::{FakeClaude, SAMPLE_CV, make_job, make_job_with, sample_dossier_json};

    // Two instants chosen like the ones in the window tests (the same schedule file is used):
    // Wednesday 2026-10-14 15:00 UTC is 11:00 in New York, a busy hour; 22:00 UTC is 18:00 there
    // and 19:00 in Santiago, so the execution window is open.
    fn busy_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 15, 0, 0).unwrap()
    }
    fn open_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 22, 0, 0).unwrap()
    }

    /// Copies the files of a `profile/` folder, except the developer's own overrides
    /// (`claude.local.toml`): those belong to one machine and must never steer a test.
    fn copy_profile(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".local.toml") {
                continue;
            }
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }

    /// A temporary project root with a copy of the real `profile/` folder.
    fn temp_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        copy_profile(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("profile"),
            &dir.path().join("profile"),
        );
        dir
    }

    struct FakeSource(Vec<Job>);

    #[async_trait::async_trait]
    impl Source for FakeSource {
        fn name(&self) -> &str {
            "fake"
        }
        async fn fetch(&self) -> FetchResult {
            FetchResult {
                jobs: self.0.clone(),
                errors: vec![],
            }
        }
    }

    struct FakeRenderer;

    #[async_trait::async_trait]
    impl Renderer for FakeRenderer {
        async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf> {
            std::fs::create_dir_all(out_path.parent().unwrap())?;
            std::fs::write(out_path, md_text)?;
            Ok(out_path.to_path_buf())
        }
    }

    /// Records the `SubmitMode` of every call so tests can check what the CLI asked for.
    struct FakeFiller(Arc<Mutex<Vec<SubmitMode>>>);

    #[async_trait::async_trait]
    impl FormFiller for FakeFiller {
        async fn submit(
            &self,
            _job: &Job,
            _cv: &Path,
            _bank: &AnswerBank,
            mode: SubmitMode,
            _shots: &Path,
        ) -> anyhow::Result<SubmitResult> {
            self.0.lock().unwrap().push(mode);
            let status = if mode == SubmitMode::DryRun {
                SubmitStatus::DryRun
            } else {
                SubmitStatus::Submitted
            };
            Ok(SubmitResult {
                status,
                reason: None,
                screenshot: None,
            })
        }
    }

    fn fake_deps(jobs: Vec<Job>) -> (Deps, Arc<Mutex<Vec<SubmitMode>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let deps = Deps {
            sources: Some(vec![Box::new(FakeSource(jobs))]),
            renderer: Box::new(FakeRenderer),
            filler: Some(Box::new(FakeFiller(calls.clone()))),
            inspector: None,
            open_checker: None,
            claude: None,
        };
        (deps, calls)
    }

    /// Runs `jobbot <parts...>` and returns the exit code and everything it printed.
    async fn cli(root: &Path, now: DateTime<Utc>, deps: Deps, parts: &[&str]) -> (i32, String) {
        let mut args = vec!["jobbot".to_string()];
        args.extend(parts.iter().map(|p| p.to_string()));
        let mut out = Vec::new();
        let code = run_with(args, root, now, deps, &mut out).await;
        (code, String::from_utf8(out).unwrap())
    }

    /// Same, for commands that never need the fakes.
    async fn plain(root: &Path, now: DateTime<Utc>, parts: &[&str]) -> (i32, String) {
        cli(root, now, fake_deps(vec![]).0, parts).await
    }

    fn tracker_of(root: &Path) -> Tracker {
        std::fs::create_dir_all(root.join("data")).unwrap();
        Tracker::open(&root.join("data").join("jobbot.db")).unwrap()
    }

    /// Puts one job in the tracker and walks it through `states`.
    fn seed(root: &Path, job: &Job, states: &[State]) {
        let t = tracker_of(root);
        t.add_job(job, 80.0, &BTreeMap::new(), open_now()).unwrap();
        t.create_application(&job.key(), open_now()).unwrap();
        for state in states {
            t.transition(&job.key(), *state, Some("falta email"), open_now())
                .unwrap();
        }
    }

    /// A prepared application with a CV file, ready for `submit`.
    fn seed_prepared(root: &Path, job: &Job) {
        seed(root, job, &[State::Prepared]);
        let cv = root.join("data").join("cv.pdf");
        std::fs::write(&cv, "pdf").unwrap();
        tracker_of(root).set_cv_path(&job.key(), &cv).unwrap();
    }

    fn write_cv_master(root: &Path, text: &str) {
        std::fs::create_dir_all(root.join("cv")).unwrap();
        std::fs::write(root.join("cv").join("cv-master.md"), text).unwrap();
    }

    const MUTATING: [&str; 4] = ["discover", "prepare", "submit", "run-daily"];

    #[tokio::test]
    async fn stop_blocks_mutating_commands() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), open_now(), &[cmd]).await;
            assert_eq!(code, 3, "{cmd}");
            assert!(out.contains("STOP activo"), "{cmd}: {out}");
        }
    }

    #[tokio::test]
    async fn outside_window_blocks_mutating_commands() {
        let root = temp_root();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), busy_now(), &[cmd]).await;
            assert_eq!(code, 4, "{cmd}");
            // The next window opens at 21:00 UTC = 18:00 in Santiago (UTC-3 in October).
            assert!(
                out.contains("Fuera de la ventana")
                    && out.contains("Próxima ventana: 2026-10-14 18:00"),
                "{cmd}: {out}"
            );
        }
        // The read-only commands do not care about the window.
        assert_eq!(plain(root.path(), busy_now(), &["status"]).await.0, 0);
        assert_eq!(plain(root.path(), busy_now(), &["digest"]).await.0, 0);
    }

    #[tokio::test]
    async fn ignore_window_overrides_the_window_not_stop() {
        let root = temp_root();
        // Busy hour, no STOP: --ignore-window lets the mutating commands through.
        let (code, _) = plain(root.path(), busy_now(), &["discover", "--ignore-window"]).await;
        assert_eq!(code, 0);
        let (code, _) = plain(root.path(), busy_now(), &["submit", "--ignore-window"]).await;
        assert_eq!(code, 0);
        // With STOP present the same flag changes nothing, in a busy hour or an open one.
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        for now in [busy_now(), open_now()] {
            for cmd in MUTATING {
                let (code, out) = plain(root.path(), now, &[cmd, "--ignore-window"]).await;
                assert_eq!(code, 3, "{cmd}");
                assert!(out.contains("STOP activo"), "{cmd}: {out}");
            }
        }
        // STOP also wins over the window when the flag is absent.
        assert_eq!(plain(root.path(), busy_now(), &["discover"]).await.0, 3);
    }

    #[tokio::test]
    async fn broken_schedule_fails_closed() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("schedule.toml"),
            "busy = []",
        )
        .unwrap();
        let (code, out) = plain(root.path(), open_now(), &["discover"]).await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Error:") && out.contains("no [[busy]] blocks"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn stop_and_resume_toggle_the_file() {
        let root = temp_root();
        assert_eq!(plain(root.path(), open_now(), &["stop"]).await.0, 0);
        assert!(root.path().join("STOP").exists());
        assert_eq!(plain(root.path(), open_now(), &["resume"]).await.0, 0);
        assert!(!root.path().join("STOP").exists());
        // Resuming when there is no STOP is not an error.
        assert_eq!(plain(root.path(), open_now(), &["resume"]).await.0, 0);
    }

    #[tokio::test]
    async fn digest_on_empty_database_writes_a_file() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["digest"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Resumen diario"), "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn status_on_empty_database() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["status"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("sin postulaciones"), "{out}");
    }

    #[tokio::test]
    async fn approve_unknown_key_is_reported() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["approve", "nope:1"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("No existe: nope:1"), "{out}");
    }

    #[tokio::test]
    async fn approve_marks_the_application() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::Prepared]);
        let (_, out) = plain(root.path(), open_now(), &["approve", "test:1"]).await;
        assert!(out.contains("Aprobada: test:1"), "{out}");
        assert!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .approved
        );
    }

    #[tokio::test]
    async fn resolve_sends_needs_gonza_back_to_the_queue() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        tracker_of(root.path())
            .set_cv_path("test:1", Path::new("cv.pdf"))
            .unwrap();
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1", "nope:1"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("De vuelta a la cola: test:1"), "{out}");
        assert!(out.contains("No existe: nope:1"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }

    #[tokio::test]
    async fn resolve_without_cv_goes_back_to_scored() {
        let root = temp_root();
        // Below-floor exceptions reach needs_gonza straight from scored, without a PDF: sending
        // them to prepared would make submit_all hand them straight back.
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1"]).await;
        assert_eq!(code, 0);
        assert!(
            out.contains("De vuelta a puntuadas (sin CV, prepare le hará el PDF): test:1"),
            "{out}"
        );
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
    }

    #[tokio::test]
    async fn confirm_marks_a_needs_gonza_application_as_submitted() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let (code, out) = plain(root.path(), open_now(), &["confirm", "test:1", "nope:1"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Confirmada como enviada: test:1"), "{out}");
        assert!(out.contains("No existe: nope:1"), "{out}");
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(
            row.approved,
            "a submission Gonza checked himself counts as reviewed"
        );
        // Only applications waiting for Gonza can be confirmed.
        seed(
            root.path(),
            &make_job_with(|j| j.external_id = "2".into()),
            &[],
        );
        let (_, out) = plain(root.path(), open_now(), &["confirm", "test:2"]).await;
        assert!(out.contains("No se pudo confirmar test:2"), "{out}");
    }

    #[tokio::test]
    async fn top_lists_scored_jobs() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["top", "--n", "3"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Acme AI — Senior AI Engineer"), "{out}");
    }

    #[tokio::test]
    async fn discover_prints_the_report_with_fake_sources() {
        let root = temp_root();
        let (deps, _) = fake_deps(vec![make_job()]);
        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Descubrimiento: 1 avisos"), "{out}");
        assert!(
            out.contains("podados: 0 vencidos, 0 fuera de región"),
            "{out}"
        );
    }

    #[test]
    fn format_report_keeps_its_counters_and_adds_the_refresh_and_the_prune() {
        let report = DiscoveryReport {
            fetched: 10,
            queued: 2,
            below_floor_flagged: 1,
            below_threshold: 3,
            excluded: 4,
            region_skipped: 5,
            stale: 6,
            already_seen: 7,
            refreshed: 8,
            pruned_stale: 9,
            pruned_region: 11,
            errors: vec!["ashby:acme: boom".into()],
        };
        assert_eq!(
            format_report(&report),
            "Descubrimiento: 10 avisos · 2 en cola · 1 bajo el piso (excepción) · 3 bajo el umbral · \
             4 excluidos · 5 fuera de región · 6 vencidos · 7 ya vistos · 8 actualizados · \
             podados: 9 vencidos, 11 fuera de región\n  ERROR ashby:acme: boom"
        );
    }

    #[tokio::test]
    async fn discover_withdraws_a_queued_posting_that_has_gone_stale_and_says_so() {
        let root = temp_root();
        let stale = make_job_with(|j| {
            j.posted_at = Some(open_now() - TimeDelta::days(135));
        });
        seed(root.path(), &stale, &[]);
        let (deps, _) = fake_deps(vec![]);

        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("podados: 1 vencidos, 0 fuera de región"),
            "{out}"
        );
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Withdrawn);
        assert!(
            row.note.as_deref().unwrap().starts_with("stale_posting:"),
            "{row:?}"
        );
    }

    #[tokio::test]
    async fn discover_rescore_prunes_too_because_its_discover_does() {
        let root = temp_root();
        let stale = make_job_with(|j| {
            j.posted_at = Some(open_now() - TimeDelta::days(135));
        });
        seed(root.path(), &stale, &[]);
        let (deps, _) = fake_deps(vec![]);

        let (_, out) = cli(root.path(), open_now(), deps, &["discover", "--rescore"]).await;

        assert!(out.contains("Repuntuación: 0 avisos revisados"), "{out}");
        assert!(
            out.contains("podados: 1 vencidos, 0 fuera de región"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn prepare_with_placeholders_exits_2_and_leaves_state_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\n⟦ completar resumen ⟧\n");
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 2);
        assert!(out.contains("CV no listo"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
    }

    #[tokio::test]
    async fn prepare_renders_with_the_injected_renderer() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("1 postulaciones preparadas"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }

    #[tokio::test]
    async fn submit_is_dry_run_unless_live() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);
        // Approved, so the review gate would let a live submission through.
        tracker_of(root.path()).approve(&job.key()).unwrap();

        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("prueba en seco"), "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun]);

        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--live"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::Live]);
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Submitted
        );
    }

    /// Says the same thing about every posting.
    struct FixedChecker(OpenStatus);

    #[async_trait::async_trait]
    impl OpenChecker for FixedChecker {
        async fn check(&self, _job: &Job) -> OpenStatus {
            self.0
        }
    }

    /// A source that is down: no postings, one error.
    struct DownSource;

    #[async_trait::async_trait]
    impl Source for DownSource {
        fn name(&self) -> &str {
            "down"
        }
        async fn fetch(&self) -> FetchResult {
            FetchResult {
                jobs: vec![],
                errors: vec!["down: HTTP 503".to_string()],
            }
        }
    }

    #[tokio::test]
    async fn a_discover_where_every_source_failed_is_not_counted_as_done() {
        let root = temp_root();
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(DownSource)]);

        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;

        assert_eq!(code, 0, "{out}");
        let stamp = tracker_of(root.path())
            .get_meta("last_discover_at")
            .unwrap();
        assert_eq!(
            stamp, None,
            "run-daily must try again, not wait {DISCOVER_COOLDOWN_HOURS} h"
        );

        // A run that fetched something is done even when one source had an error.
        let (mut deps, _) = fake_deps(vec![make_job()]);
        deps.sources = Some(vec![
            Box::new(DownSource),
            Box::new(FakeSource(vec![make_job()])),
        ]);
        cli(root.path(), open_now(), deps, &["discover"]).await;
        let stamp = tracker_of(root.path())
            .get_meta("last_discover_at")
            .unwrap();
        assert!(stamp.is_some());
    }

    #[tokio::test]
    async fn run_daily_writes_the_digest_even_when_a_step_fails() {
        let root = temp_root();
        // A step that cannot even start (a broken profile file) must not cost Gonza the digest.
        std::fs::write(
            root.path().join("profile").join("criteria.toml"),
            "this is = not [valid",
        )
        .unwrap();

        let (code, out) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR, "{out}");
        assert!(out.contains("criteria"), "the failure is shown: {out}");
        let digests = std::fs::read_dir(root.path().join("data"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("digest-"))
            .count();
        assert_eq!(digests, 1, "the digest was written: {out}");
    }

    #[tokio::test]
    async fn submit_withdraws_a_posting_its_source_no_longer_lists() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);

        let (mut deps, calls) = fake_deps(vec![]);
        deps.open_checker = Some(Box::new(FixedChecker(OpenStatus::Closed)));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("posting_closed"), "{out}");
        assert!(calls.lock().unwrap().is_empty(), "no form was opened");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Withdrawn
        );
    }

    #[tokio::test]
    async fn submit_goes_on_when_the_source_cannot_say() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);

        let (mut deps, calls) = fake_deps(vec![]);
        deps.open_checker = Some(Box::new(FixedChecker(OpenStatus::Unknown)));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;

        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun]);
    }

    #[tokio::test]
    async fn submit_assist_uses_assist_mode_and_approves() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job); // not approved: in Live mode this would be a dry run
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--assist"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Modo asistido"), "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::Assist]);
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(row.approved);
        // --live and --assist exclude each other.
        assert_eq!(
            plain(root.path(), open_now(), &["submit", "--live", "--assist"])
                .await
                .0,
            1
        );
    }

    /// A filler whose dry run always stops for a question the bank cannot answer.
    struct LacksAnswersFiller;

    #[async_trait::async_trait]
    impl FormFiller for LacksAnswersFiller {
        async fn submit(
            &self,
            _job: &Job,
            _cv: &Path,
            _bank: &AnswerBank,
            _mode: SubmitMode,
            _shots: &Path,
        ) -> anyhow::Result<SubmitResult> {
            Ok(SubmitResult {
                status: SubmitStatus::NeedsGonza,
                reason: Some("faltan respuestas: Why are you interested in this role? *".into()),
                screenshot: None,
            })
        }
    }

    #[tokio::test]
    async fn submit_points_to_assist_when_a_form_lacks_answers() {
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (mut deps, _calls) = fake_deps(vec![]);
        deps.filler = Some(Box::new(LacksAnswersFiller));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("faltan respuestas: Why are you interested in this role? *"),
            "{out}"
        );
        // The way to answer them yourself: back to the queue, then the assisted mode.
        assert!(
            out.contains("jobbot resolve KEY") && out.contains("jobbot submit --assist"),
            "{out}"
        );
        // A run with nothing of the kind does not print the hint.
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (deps, _calls) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert!(!out.contains("jobbot submit --assist"), "{out}");
    }

    #[tokio::test]
    async fn run_daily_keeps_unreviewed_applications_in_dry_run() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Prepared but NOT approved: even though run-daily submits live, the review gate must
        // turn this into a dry run.
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "9".into()));
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun], "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_with_placeholder_cv_skips_submit_but_writes_the_digest() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 2, "{out}");
        assert!(calls.lock().unwrap().is_empty());
        assert!(out.contains("se omite el envío"), "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

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
    async fn discover_rescore_requeues_stored_jobs() {
        let root = temp_root();
        // A job stored with a low score and no application, as an earlier criteria file left it.
        // `make_job()`'s one-line description scores under the shipped threshold, so this one
        // carries a description that reaches it (the same text the discovery tests use).
        let strong = make_job_with(|j| {
            j.description = "Build LLM agents with python, typescript, react, rag and mcp. \
                             Applied AI and generative AI work on agentic workflows, with an \
                             engineering culture and open source time. \
                             Full-time position with mentorship and a take-home exercise."
                .into();
        });
        tracker_of(root.path())
            .add_job(&strong, 10.0, &BTreeMap::new(), open_now())
            .unwrap();
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["discover", "--rescore"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Repuntuación: 1 avisos revisados · 1 nuevos en cola"),
            "{out}"
        );
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
        // Plain discover never touches stored scores.
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert!(!out.contains("Repuntuación"), "{out}");
    }

    #[tokio::test]
    async fn run_daily_skips_discovery_within_the_cooldown() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Descubrimiento: 0 avisos"), "{out}");
        // The same hour again (the task fires hourly): the sources are not fetched a second time...
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Descubrimiento omitido") && !out.contains("Descubrimiento: 0 avisos"),
            "{out}"
        );
        // ...but an explicit discover always runs.
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert!(out.contains("Descubrimiento: 0 avisos"), "{out}");
    }

    #[tokio::test]
    async fn prep_flow_next_reveal_grade_stats() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "next"]).await;
        assert_eq!(code, 0, "{out}");
        // The first line is "<id> [<topic>]".
        let qid = out
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();

        let (code, out) = plain(root.path(), open_now(), &["prep", "reveal", qid.as_str()]).await;
        assert_eq!(code, 0);
        assert!(!out.trim().is_empty());

        let (code, out) = plain(
            root.path(),
            open_now(),
            &["prep", "grade", qid.as_str(), "4"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("nota 4 registrada"), "{out}");

        // Grades outside 0-5 are a usage error: clap rejects them before anything is stored.
        let (code, _) = plain(
            root.path(),
            open_now(),
            &["prep", "grade", qid.as_str(), "9"],
        )
        .await;
        assert_eq!(code, 1);

        let (code, out) = plain(root.path(), open_now(), &["prep", "stats"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Vistas: 1/"), "{out}");
    }

    #[tokio::test]
    async fn prep_next_can_filter_by_topic() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "next", "--topic", "sql"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.lines().next().unwrap().ends_with("[sql]"), "{out}");
        let (_, out) = plain(
            root.path(),
            open_now(),
            &["prep", "next", "--topic", "no-such-topic"],
        )
        .await;
        assert!(out.contains("Estás al día"), "{out}");
    }

    #[tokio::test]
    async fn prep_unknown_question_is_reported() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "reveal", "nope"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("Pregunta desconocida: nope"), "{out}");
        let (code, _) = plain(root.path(), open_now(), &["prep", "grade", "nope", "3"]).await;
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn prep_stats_lists_weak_topics_and_ignores_stop_and_window() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        // A low grade makes the topic weak. The study commands run even with STOP and in a busy hour.
        let (_, first) = plain(
            root.path(),
            busy_now(),
            &["prep", "next", "--topic", "javascript"],
        )
        .await;
        let qid = first
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        let (code, _) = plain(
            root.path(),
            busy_now(),
            &["prep", "grade", qid.as_str(), "1"],
        )
        .await;
        assert_eq!(code, 0);
        let (code, out) = plain(root.path(), busy_now(), &["prep", "stats"]).await;
        assert_eq!(code, 0);
        assert!(
            out.contains("Tema débil: javascript (promedio 1.0)"),
            "{out}"
        );
    }

    // ---- Final review fixes ----

    #[tokio::test]
    async fn a_second_run_is_refused_while_another_one_holds_the_run_lock() {
        let root = temp_root();
        std::fs::create_dir_all(root.path().join("data")).unwrap();
        // Another jobbot process (an assisted session, the hourly task) holds the lock file.
        let held = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(root.path().join("data").join("jobbot.lock"))
            .unwrap();
        held.lock().unwrap();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), open_now(), &[cmd]).await;
            assert_eq!(code, 1, "{cmd}: {out}");
            assert!(out.contains("Otra ejecución"), "{cmd}: {out}");
        }
        // Study and bookkeeping commands never take the lock.
        assert_eq!(plain(root.path(), open_now(), &["status"]).await.0, 0);
        drop(held);
        assert_eq!(plain(root.path(), open_now(), &["discover"]).await.0, 0);
    }

    /// A board that is slow to answer, so a run lasts long enough to cross the edge of the window.
    struct SlowSource(std::time::Duration);

    #[async_trait::async_trait]
    impl Source for SlowSource {
        fn name(&self) -> &str {
            "slow"
        }
        async fn fetch(&self) -> FetchResult {
            tokio::time::sleep(self.0).await;
            FetchResult {
                jobs: vec![],
                errors: vec![],
            }
        }
    }

    #[tokio::test]
    async fn run_daily_stops_when_the_window_closes_during_the_run() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Prepared and not approved: if the run reached `submit` it would dry-run this one.
        seed_prepared(root.path(), &make_job());
        // 0.4 s before Santiago's busy block starts (09:00 CLST = 12:00 UTC); discovery takes 1.2 s.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 59).unwrap() + TimeDelta::milliseconds(600);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let deps = Deps {
            sources: Some(vec![Box::new(SlowSource(
                std::time::Duration::from_millis(1200),
            ))]),
            renderer: Box::new(FakeRenderer),
            filler: Some(Box::new(FakeFiller(calls.clone()))),
            inspector: None,
            open_checker: None,
            claude: None,
        };
        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;
        // Not an error and not exit 4: run-daily.cmd would throw the log of exit 4 away.
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("La ventana de ejecución se cerró"), "{out}");
        assert!(
            calls.lock().unwrap().is_empty(),
            "nothing is submitted once the window has closed: {out}"
        );
        // The digest is still written.
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn resolve_clears_the_old_screenshot_so_the_next_dry_run_takes_a_fresh_one() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);
        // A dry run stopped for a missing answer and left its screenshot behind.
        let tracker = tracker_of(root.path());
        tracker
            .set_screenshot(&job.key(), Path::new("old.png"))
            .unwrap();
        tracker
            .transition(
                &job.key(),
                State::NeedsGonza,
                Some("faltan respuestas: Email"),
                open_now(),
            )
            .unwrap();
        drop(tracker);
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Prepared);
        assert_eq!(row.screenshot_path, None);
        // The corrected form gets a fresh dry run instead of "Nada que enviar.".
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun], "{out}");
    }

    /// `deps` with a fake Claude whose handle the test keeps, to read the requests afterwards.
    fn with_claude(mut deps: Deps, fake: &Arc<FakeClaude>) -> Deps {
        let runner: Arc<dyn ClaudeRunner> = fake.clone();
        deps.claude = Some(runner);
        deps
    }

    fn write_claude_toml(root: &Path, text: &str) {
        std::fs::write(root.join("profile").join("claude.toml"), text).unwrap();
    }

    #[tokio::test]
    async fn claude_check_asks_once_and_reports_the_spend() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0123)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Respuesta: OK") && out.contains("US$0.0123"),
            "{out}"
        );
        assert!(out.contains("de US$1.50"), "{out}");
        assert_eq!(fake.requests().len(), 1);
        let ledger = tracker_of(root.path())
            .claude_calls_by_stage(open_now() - TimeDelta::hours(1))
            .unwrap();
        assert_eq!(ledger["check"].0, 1);
    }

    #[tokio::test]
    async fn claude_check_respects_stop_and_the_window() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let (code, _) = cli(
            root.path(),
            busy_now(),
            with_claude(fake_deps(vec![]).0, &fake),
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 4);
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, _) = cli(
            root.path(),
            open_now(),
            with_claude(fake_deps(vec![]).0, &fake),
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 3);
        assert!(
            fake.requests().is_empty(),
            "no call may be made while blocked"
        );
    }

    #[tokio::test]
    async fn claude_check_says_when_claude_is_not_there() {
        let root = temp_root();
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(
            root.path(),
            open_now(),
            fake_deps(vec![]).0,
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("no se encontró claude"), "{out}");
    }

    #[tokio::test]
    async fn claude_check_stops_when_the_budget_is_spent() {
        let root = temp_root();
        write_claude_toml(root.path(), "daily_budget_usd = 0.01\n");
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 0.02, 1, 1, true)
            .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(fake.requests().is_empty());
    }

    /// The JSON `claude_account_last` of the tracker, as the panel and the digest will read it.
    fn account_record(root: &Path) -> serde_json::Value {
        let text = tracker_of(root)
            .get_meta("claude_account_last")
            .unwrap()
            .expect("the preflight was recorded");
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn claude_check_prints_the_account_the_calls_use_and_records_it() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.01)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Cuenta: método claude.ai · proveedor firstParty · suscripción max")
                && out.contains("correo t***@example.com · org org-test · directorio"),
            "{out}"
        );
        let record = account_record(root.path());
        assert_eq!(record["ok"], serde_json::json!(true));
        assert_eq!(record["cli_version"], serde_json::json!("2.1.284"));
        assert_eq!(
            tracker_of(root.path())
                .get_meta("claude_account_last_check")
                .unwrap()
                .as_deref(),
            Some(open_now().to_rfc3339().as_str())
        );
    }

    #[tokio::test]
    async fn claude_check_stops_before_the_call_when_the_login_is_not_private() {
        let root = temp_root();
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::text("OK", 0.01)])
                .refusing_account("la credencial activa es api_key, no la suscripción personal"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("es api_key"), "{out}");
        assert!(fake.requests().is_empty(), "no prompt may go out");
        assert_eq!(account_record(root.path())["ok"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn run_daily_fails_before_any_prompt_when_the_login_is_not_private() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .refusing_account("la cuenta privada de jobbot no tiene sesión"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("Claude no se usó") && out.contains("no tiene sesión"),
            "{out}"
        );
        assert!(fake.requests().is_empty(), "run-daily never called -p");
        assert!(!dossier_file(root.path(), "test:1").exists());
        // The digest is still written: it is what Gonza reads to learn why the run stopped.
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
        let record = account_record(root.path());
        assert_eq!(record["ok"], serde_json::json!(false));
        assert!(
            record["message"]
                .as_str()
                .unwrap()
                .contains("no tiene sesión")
        );
    }

    #[tokio::test]
    async fn dossier_stops_before_any_prompt_when_the_login_is_not_private() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .refusing_account("la suscripción activa es team"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["dossier", "test:1"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("es team"), "{out}");
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn claude_login_with_an_injected_runner_only_verifies_the_login() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Sesión verificada") && out.contains("suscripción max"),
            "{out}"
        );
        // The shipped file leaves the address and the org empty: say how to pin them, in the
        // git-ignored local file with the orgId already filled in, never in the committed one.
        assert!(
            out.contains("profile/claude.local.toml")
                && out.contains("expected_email = ")
                && out.contains("expected_org_id = \"org-test\""),
            "{out}"
        );
        assert!(
            !out.contains("de profile/claude.toml"),
            "claude.toml is committed: {out}"
        );
        assert_eq!(account_record(root.path())["ok"], serde_json::json!(true));

        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        let deps = with_claude(fake_deps(vec![]).0, &refused);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("La sesión no sirve") && out.contains("cuenta equivocada"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn auto_approve_shows_sets_and_turns_off_the_threshold_and_status_says_it() {
        let root = temp_root();
        let run = |parts: Vec<&'static str>| {
            let path = root.path().to_path_buf();
            async move { plain(&path, open_now(), &parts).await }
        };
        let (code, shown) = run(vec!["auto-approve"]).await;
        assert_eq!(code, 0, "{shown}");
        assert!(shown.contains("apagada"), "{shown}");

        let (code, set) = run(vec!["auto-approve", "90"]).await;
        assert_eq!(code, 0, "{set}");
        assert!(set.contains("desde 90 puntos"), "{set}");
        assert!(
            run(vec!["auto-approve"])
                .await
                .1
                .contains("desde 90 puntos")
        );
        let (_, status) = run(vec!["status"]).await;
        assert!(
            status.contains("Auto-aprobación: desde 90 puntos"),
            "{status}"
        );

        // Out of range or not a number: exit 1, the reason, and the 90 stays.
        for bad in ["59", "101", "-4", "nueve", "85.5", ""] {
            let (code, out) = plain(root.path(), open_now(), &["auto-approve", bad]).await;
            assert_eq!(code, 1, "{bad:?}: {out}");
            assert!(
                out.contains("60") || out.contains("entero"),
                "{bad:?}: {out}"
            );
        }
        assert!(
            run(vec!["auto-approve"])
                .await
                .1
                .contains("desde 90 puntos")
        );

        // It only writes the tracker: STOP and the execution window do not matter.
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, out) = cli(
            root.path(),
            busy_now(),
            fake_deps(vec![]).0,
            &["auto-approve", "off"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("apagada"), "{out}");
        let (_, status) = run(vec!["status"]).await;
        assert!(status.contains("Auto-aprobación: apagada"), "{status}");
    }

    #[tokio::test]
    async fn status_claude_check_and_login_say_when_the_account_is_not_anchored() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, check) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{check}");
        let (_, status) = plain(root.path(), open_now(), &["status"]).await;
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (_, login) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        for (what, out) in [
            ("claude-check", &check),
            ("status", &status),
            ("claude-login", &login),
        ] {
            assert!(
                out.contains("cuenta sin anclar") && out.contains("claude.local.toml"),
                "{what}: {out}"
            );
        }

        // Both keys fixed in the local file: nothing to warn about.
        std::fs::write(
            root.path().join("profile").join("claude.local.toml"),
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-test\"\n",
        )
        .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, check) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{check}");
        let (_, status) = plain(root.path(), open_now(), &["status"]).await;
        for (what, out) in [("claude-check", &check), ("status", &status)] {
            assert!(!out.contains("sin anclar"), "{what}: {out}");
        }
    }

    #[tokio::test]
    async fn a_login_the_preflight_refuses_is_closed_so_no_credentials_stay_behind() {
        let root = temp_root();
        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        let deps = with_claude(fake_deps(vec![]).0, &refused);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("cuenta equivocada"), "{out}");
        assert_eq!(refused.logouts(), 1, "{out}");
        assert!(
            out.contains("Cerré esa sesión") && out.contains("ventana privada"),
            "{out}"
        );

        // A login that passes is left alone.
        let accepted = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &accepted);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(accepted.logouts(), 0, "{out}");
        assert!(!out.contains("Cerré esa sesión"), "{out}");
    }

    #[test]
    fn the_notice_before_the_browser_opens_asks_for_a_private_window_and_never_prints_the_address()
    {
        let plain = login_notice(
            &ClaudeConfig::default(),
            Path::new("C:/jobbot/claude-config"),
        );
        assert!(plain.contains("ventana privada"), "{plain}");
        assert!(plain.contains("C:/jobbot/claude-config"), "{plain}");
        assert!(plain.contains("empresa"), "the employer is named: {plain}");

        let fixed = login_notice(
            &ClaudeConfig {
                expected_email: "me@example.com".into(),
                ..ClaudeConfig::default()
            },
            Path::new("C:/jobbot/claude-config"),
        );
        assert!(fixed.contains("ventana privada"), "{fixed}");
        assert!(fixed.contains("m***@example.com"), "{fixed}");
        assert!(!fixed.contains("me@example.com"), "{fixed}");
    }

    #[tokio::test]
    async fn a_managed_policy_is_a_warning_in_claude_check_login_and_status_never_a_refusal() {
        let root = temp_root();
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]).warning_about(note));

        // claude-check: the verdict is the same, the warning is one more line.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
        assert!(
            out.contains("Respuesta: OK"),
            "the call still went out: {out}"
        );

        // It is kept with the verdict, so `status` can show it later without asking again.
        assert_eq!(
            account_record(root.path())["warnings"],
            serde_json::json!([note])
        );
        let (_, out) = plain(root.path(), open_now(), &["status"]).await;
        assert!(
            out.contains("Cuenta de Claude: verificada") && out.contains(&format!("Aviso: {note}")),
            "{out}"
        );

        // claude-login prints it too, and still succeeds.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
    }

    #[tokio::test]
    async fn run_daily_prints_the_policy_warning_when_it_asks_claude_for_dossiers() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .warning_about(note),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
    }

    #[tokio::test]
    async fn claude_login_stops_suggesting_once_the_local_file_pins_the_account() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("claude.local.toml"),
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-test\"\n",
        )
        .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Sesión verificada"), "{out}");
        assert!(!out.contains("Sugerencia"), "the keys are set: {out}");
        assert!(
            !out.contains("me@example.com"),
            "the full address is never printed: {out}"
        );
    }

    #[test]
    fn the_test_projects_do_not_copy_the_developers_local_profile_files() {
        let from = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("claude.toml"), "model = \"m\"\n").unwrap();
        std::fs::write(
            from.path().join("claude.local.toml"),
            "expected_email = \"x\"\n",
        )
        .unwrap();
        let to = tempfile::tempdir().unwrap();

        copy_profile(from.path(), &to.path().join("profile"));

        let mut copied: Vec<String> = std::fs::read_dir(to.path().join("profile"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        copied.sort();
        assert_eq!(copied, vec!["claude.toml".to_string()]);
    }

    #[tokio::test]
    async fn claude_login_ignores_stop_and_the_window() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), busy_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
    }

    #[tokio::test]
    async fn prep_next_serves_the_cards_that_dossiers_add() {
        let root = temp_root();
        std::fs::create_dir_all(root.path().join("data")).unwrap();
        let store = PrepStore::open(&root.path().join("data").join("prep.db")).unwrap();
        let card = crate::prep::Question {
            id: "gap-x-1".into(),
            topic: "kubernetes".into(),
            question: "What does a Deployment do?".into(),
            answer: "It manages ReplicaSets.".into(),
        };
        store.add_cards(&[card], "test:1", open_now()).unwrap();
        drop(store);
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["prep", "next", "--topic", "kubernetes"],
        )
        .await;
        assert_eq!(code, 0);
        assert!(
            out.contains("gap-x-1 [kubernetes]") && out.contains("What does a Deployment do?"),
            "{out}"
        );
        let (_, out) = plain(root.path(), open_now(), &["prep", "reveal", "gap-x-1"]).await;
        assert!(out.contains("It manages ReplicaSets."), "{out}");
    }

    /// A Claude that takes its time, so a run lasts long enough to cross the edge of the window.
    struct SlowClaude {
        delay: std::time::Duration,
        inner: Arc<FakeClaude>,
    }

    #[async_trait::async_trait]
    impl ClaudeRunner for SlowClaude {
        async fn run(&self, req: &ClaudeRequest) -> Result<ClaudeReply, ClaudeError> {
            tokio::time::sleep(self.delay).await;
            self.inner.run(req).await
        }

        async fn preflight(&self) -> Result<crate::claude::AccountSeen, ClaudeError> {
            self.inner.preflight().await
        }
    }

    fn dossier_replies(n: usize) -> Arc<FakeClaude> {
        Arc::new(FakeClaude::new(
            (0..n)
                .map(|_| FakeClaude::structured(sample_dossier_json(), 0.05))
                .collect(),
        ))
    }

    fn job_n(n: &str) -> Job {
        make_job_with(|j| j.external_id = n.into())
    }

    fn seed_replied(root: &Path, job: &Job) {
        seed(
            root,
            job,
            &[State::Prepared, State::Submitted, State::Replied],
        );
    }

    fn dossier_file(root: &Path, key: &str) -> PathBuf {
        dossier_path(&root.join("data").join("dossiers"), key)
    }

    #[tokio::test]
    async fn the_dossier_command_writes_the_file_and_the_drill_cards() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["dossier", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Dossier listo"), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
        let (_, card) = plain(
            root.path(),
            open_now(),
            &["prep", "next", "--topic", "kubernetes-operators"],
        )
        .await;
        assert!(
            card.contains(&format!("gap-{}-1", crate::outbound::key_slug("test:1"))),
            "{card}"
        );
    }

    #[tokio::test]
    async fn the_dossier_command_reports_unknown_keys_and_keeps_going() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(
            root.path(),
            open_now(),
            deps,
            &["dossier", "nope:1", "test:1"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Aviso desconocido: nope:1") && out.contains("Dossier listo"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn the_dossier_command_respects_stop_the_window_and_the_budget() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        assert_eq!(
            cli(root.path(), busy_now(), go(), &["dossier", "test:1"])
                .await
                .0,
            4
        );
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(
            cli(root.path(), open_now(), go(), &["dossier", "test:1"])
                .await
                .0,
            3
        );
        std::fs::remove_file(root.path().join("STOP")).unwrap();
        write_claude_toml(root.path(), "daily_budget_usd = 0.01\n");
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 0.02, 1, 1, true)
            .unwrap();
        let (code, out) = cli(root.path(), open_now(), go(), &["dossier", "test:1"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(
            fake.requests().is_empty(),
            "no call was made in any of the three cases"
        );
    }

    #[tokio::test]
    async fn run_daily_writes_the_dossier_of_a_replied_application() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Dossier listo"), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
        assert_eq!(fake.requests().len(), 1);
    }

    #[tokio::test]
    async fn run_daily_writes_at_most_the_configured_number_of_dossiers() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        for n in ["1", "2", "3"] {
            seed_replied(root.path(), &job_n(n));
        }
        write_claude_toml(root.path(), "max_dossiers_per_run = 2\n");
        let fake = dossier_replies(3);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(fake.requests().len(), 2, "{out}");
        // The next run does the third one: the first two are not asked again.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        cli(
            root.path(),
            open_now() + TimeDelta::hours(1),
            deps,
            &["run-daily"],
        )
        .await;
        assert_eq!(fake.requests().len(), 3);
    }

    #[tokio::test]
    async fn run_daily_skips_dossiers_when_the_budget_is_spent_and_still_exits_zero() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 5.0, 1, 1, true)
            .unwrap();
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(fake.requests().is_empty());
        assert!(!dossier_file(root.path(), "test:1").exists());
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_does_not_fail_when_claude_is_missing() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Dossiers omitidos") && out.contains("no se encontró claude"),
            "{out}"
        );
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_never_touches_claude_when_nobody_has_replied() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_prepared(root.path(), &make_job());
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            !out.contains("Dossiers omitidos"),
            "no replied application, no reason to look for claude: {out}"
        );
    }

    #[tokio::test]
    async fn mark_explains_in_spanish_and_points_to_confirm_for_needs_gonza() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::NeedsGonza],
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "replied"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("no se puede pasar de needs_gonza a replied") && out.contains("confirm"),
            "{out}"
        );
        assert!(!out.contains("illegal transition"), "{out}");
    }

    #[tokio::test]
    async fn panel_help_documents_its_flags() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["panel", "--help"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("--port") && out.contains("--no-open"), "{out}");
    }

    #[tokio::test]
    async fn panel_refuses_a_port_that_is_not_a_port() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["panel", "--port", "70000"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("--port") && out.contains("70000"),
            "the refusal should name the flag and the value, not just fail: {out}"
        );
    }

    #[tokio::test]
    async fn the_main_help_lists_the_panel() {
        let root = temp_root();
        let (_, out) = plain(root.path(), open_now(), &["--help"]).await;
        assert!(out.contains("panel"), "{out}");
    }

    #[tokio::test]
    async fn stop_names_every_command_it_stops() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["stop"]).await;
        assert_eq!(code, 0, "{out}");
        for name in [
            "discover",
            "prepare",
            "submit",
            "run-daily",
            "dossier",
            "freelance",
            "claude-check",
        ] {
            assert!(out.contains(name), "STOP message should name {name}: {out}");
        }
    }

    #[tokio::test]
    async fn a_broken_claude_toml_is_not_reported_when_nobody_has_replied() {
        let root = temp_root();
        // With a CV master the earlier steps pass and `run-daily` reaches the dossier step.
        write_cv_master(root.path(), SAMPLE_CV);
        write_claude_toml(root.path(), "this is = not [valid toml\n");
        let (_, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert!(
            !out.contains("Dossiers omitidos"),
            "nothing was due, so claude.toml should not even be read: {out}"
        );
    }

    #[tokio::test]
    async fn run_daily_stops_the_dossiers_when_the_window_closes_in_the_middle() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &job_n("1"));
        seed_replied(root.path(), &job_n("2"));
        // 1.5 s before Santiago's busy block starts (12:00 UTC); each Claude call takes 2 s. The
        // margin is generous on purpose: the steps before the dossiers must finish inside it.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 58).unwrap() + TimeDelta::milliseconds(500);
        let fake = dossier_replies(2);
        let slow = SlowClaude {
            delay: std::time::Duration::from_secs(2),
            inner: fake.clone(),
        };
        let mut deps = fake_deps(vec![]).0;
        let runner: Arc<dyn ClaudeRunner> = Arc::new(slow);
        deps.claude = Some(runner);
        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            fake.requests().len(),
            1,
            "the second dossier waits for the next window: {out}"
        );
        assert!(out.contains("se detiene"), "{out}");
    }

    #[tokio::test]
    async fn mark_records_a_reply_by_hand_and_it_shows_in_the_status() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::Submitted],
        );
        let (code, out) = plain(root.path(), busy_now(), &["mark", "test:1", "replied"]).await;
        assert_eq!(code, 0, "mark needs no window: {out}");
        assert!(out.contains("test:1 → replied"), "{out}");
        let (code, _) = plain(
            root.path(),
            busy_now(),
            &["mark", "test:1", "interview", "--note", "viernes 19:00"],
        )
        .await;
        assert_eq!(code, 0);
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Interview);
        assert!(
            plain(root.path(), busy_now(), &["status"])
                .await
                .1
                .contains("interview")
        );
    }

    #[tokio::test]
    async fn record_registers_a_hand_sent_application_with_its_real_time_inside_work_hours() {
        let root = temp_root();
        let (code, out) = plain(
            root.path(),
            busy_now(),
            &[
                "record",
                "--company",
                "Globex",
                "--title",
                "Senior Software Engineer",
                "--url",
                "https://jobs.ashbyhq.com/globex/abc/application",
                "--at",
                "2026-10-05T05:21:42Z",
                "--cv",
                "data/cv/globex.pdf",
                "--note",
                "enviada con Playwright",
                "--salary-min",
                "5000",
            ],
        )
        .await;
        assert_eq!(code, 0, "record needs no window: {out}");
        assert!(out.contains("Registrada como enviada: Globex"), "{out}");
        let tracker = tracker_of(root.path());
        let row = tracker
            .get_application("manual:jobs.ashbyhq.com_globex_abc")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert_eq!(
            row.submitted_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 5, 5, 21, 42).unwrap())
        );
        assert_eq!(row.cv_path.as_deref(), Some("data/cv/globex.pdf"));
        assert_eq!(row.note.as_deref(), Some("enviada con Playwright"));
        assert!(!row.approved);
        assert!(
            plain(root.path(), busy_now(), &["status"])
                .await
                .1
                .contains("submitted")
        );
    }

    #[tokio::test]
    async fn record_ignores_stop_and_stamps_now_when_no_time_is_given() {
        let root = temp_root();
        assert_eq!(plain(root.path(), open_now(), &["stop"]).await.0, 0);
        let (code, out) = plain(
            root.path(),
            open_now(),
            &[
                "record",
                "--company",
                "Initech",
                "--title",
                "Senior Full-Stack Engineer",
                "--url",
                "https://jobs.ashbyhq.com/initech/xyz",
            ],
        )
        .await;
        assert_eq!(code, 0, "recording a fact is not blocked by STOP: {out}");
        let row = tracker_of(root.path())
            .get_application("manual:jobs.ashbyhq.com_initech_xyz")
            .unwrap()
            .unwrap();
        assert_eq!(row.submitted_at, Some(open_now()));
    }

    #[tokio::test]
    async fn record_refuses_a_bad_or_future_time_and_a_key_mixed_with_company() {
        let root = temp_root();
        let base = [
            "record",
            "--company",
            "Globex",
            "--title",
            "SE",
            "--url",
            "https://jobs.ashbyhq.com/globex/abc",
        ];
        let with = |extra: &[&'static str]| {
            let mut parts: Vec<&str> = base.to_vec();
            parts.extend_from_slice(extra);
            parts
        };
        let (code, out) = plain(root.path(), open_now(), &with(&["--at", "ayer"])).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("--at") && out.contains("RFC 3339"), "{out}");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &with(&["--at", "2027-01-01T00:00:00Z"]),
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("--at") && out.contains("futuro"), "{out}");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["record", "--key", "test:1", "--company", "Globex"],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(
            tracker_of(root.path()).counts().unwrap().is_empty(),
            "none of the refusals stored anything"
        );
    }

    #[tokio::test]
    async fn record_of_a_stored_key_works_once_and_a_replay_exits_with_an_error() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::Prepared]);
        let args = ["record", "--key", "test:1"];
        let (code, out) = plain(root.path(), open_now(), &args).await;
        assert_eq!(code, 0, "{out}");
        let (code, out) = plain(root.path(), open_now(), &args).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("ya está registrada"), "{out}");
    }

    #[tokio::test]
    async fn mark_refuses_states_it_does_not_own_illegal_moves_and_unknown_keys() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::Submitted],
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "prepared"]).await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Estado no válido") && out.contains("replied"),
            "{out}"
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "offer"]).await;
        assert_eq!(code, 1, "submitted cannot jump to an offer");
        assert!(out.contains("No se pudo marcar"), "{out}");
        let (code, out) = plain(root.path(), open_now(), &["mark", "nope:1", "replied"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("No se pudo marcar"), "{out}");
    }

    /// Writes `value` as the import file of the inbox and returns its path.
    fn write_inbox_file(root: &Path, value: &serde_json::Value) -> PathBuf {
        let file = root.join("correos.json");
        std::fs::write(&file, value.to_string()).unwrap();
        file
    }

    /// A valid message received two hours before `open_now()`.
    fn inbox_msg(id: &str, kind: &str) -> serde_json::Value {
        serde_json::json!({
            "message_id": id,
            "thread_id": format!("th-{id}"),
            "from": "Ana <ana@acme.example>",
            "subject": format!("Asunto {id}"),
            "received_at": "2026-10-14T20:00:00Z",
            "kind": kind,
            "summary": format!("Resumen {id}"),
        })
    }

    #[tokio::test]
    async fn inbox_import_writes_the_tracker_and_ignores_stop_and_the_window() {
        let root = temp_root();
        // Sent three days before the messages arrive.
        let job = make_job();
        let t = tracker_of(root.path());
        let sent = open_now() - TimeDelta::days(3);
        t.add_job(&job, 80.0, &BTreeMap::new(), sent).unwrap();
        t.create_application(&job.key(), sent).unwrap();
        for state in [State::Prepared, State::Submitted] {
            t.transition(&job.key(), state, None, sent).unwrap();
        }
        drop(t);
        let mut rejection = inbox_msg("m1", "rejection");
        rejection["job_key"] = "test:1".into();
        let mut invite = inbox_msg("m2", "interview_invite");
        invite["company"] = "Otra Empresa".into();
        let file = write_inbox_file(root.path(), &serde_json::json!([rejection, invite]));
        let file = file.to_str().unwrap();
        // Busy hour and STOP at once: importing only writes the tracker, like `mark`.
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["inbox", "import", file]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Importados: 2")
                && out.contains("cambios de estado: 1")
                && out.contains("tareas creadas: 1")
                && out.contains("sin postulación asociada: 1"),
            "{out}"
        );
        let t = tracker_of(root.path());
        assert_eq!(
            t.get_application("test:1").unwrap().unwrap().state,
            State::Rejected
        );
        assert_eq!(
            t.open_tasks().unwrap()[0].text,
            "Responder a Otra Empresa: entrevista"
        );

        // The same file again changes nothing.
        let (code, out) = plain(root.path(), busy_now(), &["inbox", "import", file]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Importados: 0") && out.contains("omitidos (ya conocidos): 2"),
            "{out}"
        );
        assert_eq!(t.open_tasks().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn inbox_import_exits_with_an_error_when_a_message_is_rejected_but_imports_the_rest() {
        let root = temp_root();
        let file = write_inbox_file(
            root.path(),
            &serde_json::json!([inbox_msg("m1", "other"), inbox_msg("m2", "spam")]),
        );

        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;

        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("elemento 2 rechazado: tipo desconocido")
                && out.contains("Importados: 1")
                && out.contains("rechazados: 1"),
            "{out}"
        );
        assert!(tracker_of(root.path()).email_exists("m1").unwrap());
    }

    #[tokio::test]
    async fn inbox_import_says_what_is_wrong_with_a_missing_or_broken_file() {
        let root = temp_root();
        let missing = root.path().join("no-existe.json");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", missing.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("leyendo") && out.contains("no-existe.json"),
            "{out}"
        );

        let broken = root.path().join("roto.json");
        std::fs::write(&broken, "[ {").unwrap();
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", broken.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("no es JSON válido"), "{out}");
    }

    #[tokio::test]
    async fn inbox_import_refuses_a_file_that_is_too_big_or_has_too_many_messages() {
        let root = temp_root();
        // More than 2 MB in one text.
        let big = write_inbox_file(
            root.path(),
            &serde_json::json!(["x".repeat(2 * 1024 * 1024 + 1)]),
        );
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", big.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("2 MB"), "{out}");

        // 501 valid messages: one more than the limit, so none is imported.
        let many: Vec<serde_json::Value> = (0..=crate::inbox::MAX_MESSAGES)
            .map(|n| inbox_msg(&format!("m{n}"), "other"))
            .collect();
        let file = write_inbox_file(root.path(), &serde_json::Value::Array(many));
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("500") && out.contains("mensajes"), "{out}");
        assert!(!out.contains("Importados"), "nothing was imported: {out}");
        assert!(!tracker_of(root.path()).email_exists("m0").unwrap());
    }

    #[tokio::test]
    async fn today_lists_tasks_decisions_and_answers_and_needs_neither_stop_nor_the_window() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let mut reply = inbox_msg("m1", "recruiter");
        reply["summary"] = "Quiere hablar de sueldo".into();
        let mut video = inbox_msg("m2", "action_required");
        video["task"] = serde_json::json!({
            "text": "Grabar el video de Acme AI",
            "due_at": "2026-10-16T15:00:00Z"
        });
        let file = write_inbox_file(root.path(), &serde_json::json!([reply, video]));
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["today"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Tu lista de hoy — 2026-10-14")
                && out.contains("Tareas abiertas (1)")
                && out.contains("Grabar el video de Acme AI — vence en")
                && out.contains("Esperan tu decisión (1)")
                && out.contains("Acme AI — Senior AI Engineer · test:1: falta email")
                && out.contains("Respuestas recientes, últimos 7 días (2)")
                && out.contains("reclutador · «Asunto m1» — Quiere hablar de sueldo"),
            "{out}"
        );
        // Reading it changed nothing.
        assert_eq!(tracker_of(root.path()).open_tasks().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn task_add_shows_up_in_today_and_task_done_closes_it_under_stop_and_busy_hours() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(
            root.path(),
            busy_now(),
            &[
                "task",
                "add",
                "Llamar a Globex",
                "--due",
                "2026-10-16T12:00:00-03:00",
                "--job-key",
                "test:1",
                "--link",
                "https://example.com/oferta",
            ],
        )
        .await;

        assert_eq!(code, 0, "adding a task only writes the tracker: {out}");
        assert!(out.contains("Tarea agregada: #1 Llamar a Globex"), "{out}");
        let row = tracker_of(root.path()).get_task(1).unwrap().unwrap();
        assert_eq!(
            (
                row.kind.as_str(),
                row.job_key.as_deref(),
                row.link.as_deref()
            ),
            ("manual", Some("test:1"), Some("https://example.com/oferta"))
        );
        assert_eq!(
            row.due_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 16, 15, 0, 0).unwrap())
        );

        let (code, today) = plain(root.path(), busy_now(), &["today"]).await;
        assert_eq!(code, 0, "{today}");
        assert!(
            today.contains("Tareas abiertas (1)")
                && today.contains(
                    "#1 Llamar a Globex · https://example.com/oferta [test:1] — vence en 2 días"
                ),
            "{today}"
        );

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", "1"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Hecha: #1 Llamar a Globex"), "{out}");
        let (_, today) = plain(root.path(), busy_now(), &["today"]).await;
        assert!(today.contains("Tareas abiertas (0)"), "{today}");
    }

    #[tokio::test]
    async fn task_add_refuses_what_is_wrong_with_a_message_and_changes_nothing() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        let go = |extra: &'static [&'static str]| {
            let mut parts = vec!["task", "add", "Llamar a Globex"];
            parts.extend_from_slice(extra);
            parts
        };
        for (extra, needle) in [
            (
                &["--due", "mañana"][..],
                "--due debe ser una fecha RFC 3339",
            ),
            (&["--due", "1999-12-31T23:59:59Z"][..], "fuera de rango"),
            (&["--due", "9999-12-31T23:59:59Z"][..], "fuera de rango"),
            (&["--job-key", "nope:1"][..], "No existe el aviso: nope:1"),
            (&["--link", "javascript:alert(1)"][..], "enlace"),
        ] {
            let (code, out) = plain(root.path(), open_now(), &go(extra)).await;
            assert_eq!(code, 1, "{extra:?}: {out}");
            assert!(out.contains(needle), "{extra:?}: {out}");
        }
        assert!(tracker_of(root.path()).open_tasks().unwrap().is_empty());

        // Twice while the first one is open: the second is refused and nothing changes.
        assert_eq!(plain(root.path(), open_now(), &go(&[])).await.0, 0);
        let (code, out) = plain(root.path(), open_now(), &go(&[])).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("Ya tienes esa tarea abierta (#1)"), "{out}");
        assert_eq!(tracker_of(root.path()).open_tasks().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn task_done_closes_a_task_and_refuses_unknown_or_repeated_ones() {
        let root = temp_root();
        let mut item = inbox_msg("m1", "action_required");
        item["task"] = serde_json::json!({ "text": "Grabar el video" });
        let file = write_inbox_file(root.path(), &serde_json::json!([item]));
        plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        let id = tracker_of(root.path()).open_tasks().unwrap()[0]
            .id
            .to_string();
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", &id]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Hecha:") && out.contains("Grabar el video"),
            "{out}"
        );
        assert!(tracker_of(root.path()).open_tasks().unwrap().is_empty());

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", &id]).await;
        assert_eq!(code, 1);
        assert!(out.contains("ya estaba hecha"), "{out}");
        let (code, out) = plain(root.path(), busy_now(), &["task", "done", "999"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("No existe la tarea #999"), "{out}");
    }

    fn write_freelance_toml(root: &Path, text: &str) {
        std::fs::write(root.join("profile").join("freelance.toml"), text).unwrap();
    }

    fn profile_reply() -> Arc<FakeClaude> {
        Arc::new(FakeClaude::new(vec![FakeClaude::structured(
            serde_json::json!({
                "headline": "AI engineer", "bio": "I build LLM agents with Python.",
                "skills": ["Python", "Kubernetes"], "hourly_rate_usd": 20.0,
                "portfolio_ideas": [{"title": "RAG demo", "description": "Built from scratch."}]
            }),
            0.04,
        )]))
    }

    fn freelance_file(root: &Path, name: &str) -> PathBuf {
        root.join("data").join("freelance").join(name)
    }

    #[tokio::test]
    async fn freelance_refuses_until_platforms_and_a_rate_are_defined() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        let fake = profile_reply();
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        // The shipped file has no platforms.
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("Falta definir `platforms`"), "{out}");
        write_freelance_toml(root.path(), "platforms = [\"upwork\"]\n");
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("Falta definir `min_hourly_usd`"), "{out}");
        assert!(fake.requests().is_empty(), "nothing reached Claude");
    }

    #[tokio::test]
    async fn freelance_profile_writes_a_draft_and_says_nothing_was_sent() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let fake = profile_reply();
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(
            root.path(),
            open_now(),
            deps,
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Borrador listo") && out.contains("no envía nada"),
            "{out}"
        );
        let md =
            std::fs::read_to_string(freelance_file(root.path(), "upwork-profile-2026-10-14.md"))
                .unwrap();
        assert!(
            md.contains("US$35") && md.contains("descartada") && !md.contains("Kubernetes"),
            "{md}"
        );
        for secret in ["Harbor Light", "jane.doe@example.com", "+56"] {
            assert!(!fake.requests()[0].prompt.contains(secret), "{secret}");
        }
    }

    #[tokio::test]
    async fn freelance_proposal_reads_the_posting_file_and_refuses_excluded_ones() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let good = root.path().join("rag-job.txt");
        std::fs::write(&good, "Need a RAG engineer for a small internal tool.").unwrap();
        let bad = root.path().join("freight-job.txt");
        std::fs::write(&bad, "Dashboard for a freight forwarding company.").unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::structured(
            serde_json::json!({"proposal": "I would start with a spike.", "questions_for_client": ["Scope?"]}),
            0.03,
        )]));
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        let bad_path = bad.display().to_string();
        let good_path = good.display().to_string();
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "proposal", "upwork", "--job-file", &bad_path],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("El aviso cae en la lista de exclusión"),
            "{out}"
        );
        assert!(
            fake.requests().is_empty(),
            "an excluded posting never reaches Claude"
        );
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "proposal", "upwork", "--job-file", &good_path],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(freelance_file(root.path(), "proposal-upwork-2026-10-14-rag-job.md").exists());
    }

    #[tokio::test]
    async fn two_freelance_drafts_the_same_day_do_not_overwrite_each_other() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        for _ in 0..2 {
            let fake = profile_reply();
            let deps = with_claude(fake_deps(vec![]).0, &fake);
            assert_eq!(
                cli(
                    root.path(),
                    open_now(),
                    deps,
                    &["freelance", "profile", "upwork"]
                )
                .await
                .0,
                0
            );
        }
        assert!(freelance_file(root.path(), "upwork-profile-2026-10-14.md").exists());
        assert!(freelance_file(root.path(), "upwork-profile-2026-10-14-2.md").exists());
    }

    #[tokio::test]
    async fn freelance_commands_respect_stop_and_the_window() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let fake = profile_reply();
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        assert_eq!(
            cli(
                root.path(),
                busy_now(),
                go(),
                &["freelance", "profile", "upwork"]
            )
            .await
            .0,
            4
        );
        assert_eq!(
            cli(
                root.path(),
                busy_now(),
                go(),
                &["freelance", "profile", "upwork", "--ignore-window"]
            )
            .await
            .0,
            0
        );
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(
            cli(
                root.path(),
                open_now(),
                go(),
                &["freelance", "profile", "upwork", "--ignore-window"]
            )
            .await
            .0,
            3
        );
    }

    // ---- handoff (Ashby): the bot fills, Gonza sends ----

    use crate::handoff::{FieldReport, FormInspection, FormInspector, SheetField};

    const ASHBY_APPLY: &str = "https://jobs.ashbyhq.com/acme/123/application";

    fn ashby_job() -> Job {
        make_job_with(|j| j.apply_url = ASHBY_APPLY.into())
    }

    #[tokio::test]
    async fn submit_hands_an_ashby_form_off_in_every_mode() {
        for flags in [
            &["submit"][..],
            &["submit", "--live"][..],
            &["submit", "--assist"][..],
        ] {
            let root = temp_root();
            let job = ashby_job();
            seed_prepared(root.path(), &job);
            // Approved, so even Live would click on any other form.
            tracker_of(root.path()).approve(&job.key()).unwrap();
            let (deps, calls) = fake_deps(vec![]);
            let (code, out) = cli(root.path(), open_now(), deps, flags).await;
            assert_eq!(code, 0, "{flags:?}: {out}");
            assert_eq!(
                *calls.lock().unwrap(),
                vec![SubmitMode::DryRun],
                "{flags:?}"
            );
            let row = tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap();
            assert_eq!(row.state, State::NeedsGonza, "{flags:?}");
            let sheet = root.path().join("data").join("handoff").join("test_1.md");
            assert!(sheet.is_file(), "{flags:?}");
            assert!(out.contains("handoff:"), "{out}");
            assert!(out.contains("jobbot confirm KEY"), "{out}");
        }
    }

    #[tokio::test]
    async fn a_run_with_nothing_but_a_handed_off_form_is_not_nothing_to_send() {
        let root = temp_root();
        seed_prepared(root.path(), &ashby_job());
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert!(!out.contains("Nada que enviar"), "{out}");
    }

    /// An inspector that sees one required question the bank cannot answer.
    struct OneQuestionInspector;

    #[async_trait::async_trait]
    impl FormInspector for OneQuestionInspector {
        async fn inspect(
            &self,
            _job: &Job,
            _cv_pdf: &Path,
            _bank: &AnswerBank,
            _shots_dir: &Path,
        ) -> anyhow::Result<FormInspection> {
            Ok(FormInspection {
                result: SubmitResult {
                    status: SubmitStatus::NeedsGonza,
                    reason: Some("faltan respuestas: Why this role?".into()),
                    screenshot: None,
                },
                report: FieldReport::Full(vec![SheetField::missing("Why this role?")]),
            })
        }
    }

    #[tokio::test]
    async fn an_injected_inspector_does_the_dry_run_of_a_handoff_form() {
        let root = temp_root();
        seed_prepared(root.path(), &ashby_job());
        let (mut deps, calls) = fake_deps(vec![]);
        deps.inspector = Some(Box::new(OneQuestionInspector));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--live"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            calls.lock().unwrap().is_empty(),
            "the filler stayed out: {out}"
        );
        let sheet =
            std::fs::read_to_string(root.path().join("data").join("handoff").join("test_1.md"))
                .unwrap();
        assert!(
            sheet.contains("### 1. Why this role? (obligatoria)"),
            "{sheet}"
        );
    }

    #[test]
    fn a_run_of_only_handoff_forms_needs_the_inspector_and_not_the_main_browser() {
        let tracker = Tracker::in_memory().unwrap();
        let ashby = ashby_job();
        tracker
            .add_job(&ashby, 80.0, &BTreeMap::new(), open_now())
            .unwrap();
        tracker
            .create_application(&ashby.key(), open_now())
            .unwrap();
        tracker
            .transition(&ashby.key(), State::Prepared, None, open_now())
            .unwrap();
        let rows = tracker.list_by_state(State::Prepared).unwrap();
        let needs = submit_needs(&tracker, &rows, SubmitMode::Live, true).unwrap();
        assert!(!needs.filler && needs.inspector);
        // An ordinary form brings the main browser back.
        let other = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
        });
        tracker
            .add_job(&other, 70.0, &BTreeMap::new(), open_now())
            .unwrap();
        tracker
            .create_application(&other.key(), open_now())
            .unwrap();
        tracker
            .transition(&other.key(), State::Prepared, None, open_now())
            .unwrap();
        let rows = tracker.list_by_state(State::Prepared).unwrap();
        let needs = submit_needs(&tracker, &rows, SubmitMode::Live, true).unwrap();
        assert!(needs.filler && needs.inspector);
    }

    // ---- the per-company cap ----

    fn acme(n: u32) -> Job {
        make_job_with(|j| j.external_id = n.to_string())
    }

    #[tokio::test]
    async fn prepare_says_which_applications_the_company_cap_left_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        for n in 1..=3 {
            seed(root.path(), &acme(n), &[]);
        }
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("2 postulaciones preparadas"), "{out}");
        assert!(out.contains("tope por empresa"), "{out}");
        assert!(out.contains("test:3"), "{out}");
        let tracker = tracker_of(root.path());
        let state = |key: &str| tracker.get_application(key).unwrap().unwrap().state;
        assert_eq!(state("test:1"), State::Prepared);
        assert_eq!(state("test:3"), State::Scored);
    }

    #[tokio::test]
    async fn submit_reports_the_application_the_company_cap_left_alone() {
        let root = temp_root();
        for n in 1..=3 {
            seed_prepared(root.path(), &acme(n));
        }
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            *calls.lock().unwrap(),
            vec![SubmitMode::DryRun, SubmitMode::DryRun],
            "{out}"
        );
        assert!(out.contains("tope por empresa"), "{out}");
        assert!(out.contains("test:3"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:3")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }

    // ---- jobbot results ----

    #[tokio::test]
    async fn results_shows_what_submit_just_did() {
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(root.path().join("data").join("results.jsonl").is_file());
        let (code, out) = plain(root.path(), open_now(), &["results"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Últimos 1 resultados (de 1)"), "{out}");
        for expected in [
            "dry_run",
            "prepared",
            "test:1",
            "Acme AI — Senior AI Engineer",
        ] {
            assert!(out.contains(expected), "{expected} in {out}");
        }
    }

    fn log_line(root: &Path, n: u32) {
        let job = make_job_with(|j| j.external_id = n.to_string());
        let line = crate::results::ResultLine::new(
            &job,
            crate::results::AttemptMode::DryRun,
            State::Prepared,
            crate::handoff::FormTally {
                filled: Some(n as usize),
                left: vec![],
            },
            None,
            None,
            open_now(),
        );
        crate::results::append_result(&crate::results::results_path(&root.join("data")), &line)
            .unwrap();
    }

    #[tokio::test]
    async fn results_last_n_prints_only_the_latest_lines() {
        let root = temp_root();
        for n in 1..=3 {
            log_line(root.path(), n);
        }
        let (code, out) = plain(root.path(), open_now(), &["results", "--last", "2"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Últimos 2 resultados (de 3)"), "{out}");
        assert!(out.contains("test:2") && out.contains("test:3"), "{out}");
        assert!(!out.contains("test:1"), "{out}");
        // The default is the last 20: all three fit.
        let (_, out) = plain(root.path(), open_now(), &["results"]).await;
        assert!(out.contains("Últimos 3 resultados (de 3)"), "{out}");
    }

    #[tokio::test]
    async fn results_without_a_log_says_so_and_never_needs_the_window_or_stop() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, out) = plain(root.path(), busy_now(), &["results"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Sin resultados todavía"), "{out}");
        // A count of zero is not a request: it is a usage error.
        assert_eq!(
            plain(root.path(), open_now(), &["results", "--last", "0"])
                .await
                .0,
            1
        );
    }

    #[tokio::test]
    async fn top_lists_the_forms_waiting_for_gonzas_own_send() {
        let root = temp_root();
        let job = ashby_job();
        seed(root.path(), &job, &[]);
        let tracker = tracker_of(root.path());
        tracker
            .transition(
                &job.key(),
                State::NeedsGonza,
                Some("handoff: envía tú. Hoja de respuestas: data/handoff/test_1.md"),
                open_now(),
            )
            .unwrap();
        drop(tracker);
        let (code, out) = plain(root.path(), open_now(), &["top"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Para enviar tú"), "{out}");
        assert!(out.contains("Acme AI — Senior AI Engineer"), "{out}");
        assert!(out.contains("data/handoff/test_1.md"), "{out}");
        assert!(out.contains("jobbot confirm test:1"), "{out}");
        // A needs_gonza for any other reason is not listed there.
        let other = make_job_with(|j| j.external_id = "2".into());
        seed(root.path(), &other, &[State::NeedsGonza]);
        let (_, out) = plain(root.path(), open_now(), &["top"]).await;
        assert!(!out.contains("jobbot confirm test:2"), "{out}");
    }

    // ---- the record of the last run (`meta.last_run_*`), which `status` and the panel read

    /// The three keys `run-daily` leaves behind: (when, result, steps), `None` before any run.
    fn last_run(root: &Path) -> Option<(DateTime<Utc>, String, serde_json::Value)> {
        let tracker = tracker_of(root);
        let at = tracker.get_meta("last_run_at").unwrap()?;
        let result = tracker.get_meta("last_run_result").unwrap().unwrap();
        let steps = tracker.get_meta("last_run_steps").unwrap().unwrap();
        Some((
            DateTime::parse_from_rfc3339(&at)
                .unwrap()
                .with_timezone(&Utc),
            result,
            serde_json::from_str(&steps).unwrap(),
        ))
    }

    /// A posting that scores above the shipped threshold, so a discovery queues it.
    fn strong_job() -> Job {
        make_job_with(|j| {
            j.description = "Build LLM agents with python, typescript, react, rag and mcp. \
                             Applied AI and generative AI work on agentic workflows, with an \
                             engineering culture and open source time. \
                             Full-time position with mentorship and a take-home exercise."
                .into();
        })
    }

    /// A board that, while it is being read, gets a STOP file from Gonza.
    struct StopWhileFetching(PathBuf);

    #[async_trait::async_trait]
    impl Source for StopWhileFetching {
        fn name(&self) -> &str {
            "stopper"
        }
        async fn fetch(&self) -> FetchResult {
            std::fs::write(&self.0, "x").unwrap();
            FetchResult {
                jobs: vec![],
                errors: vec![],
            }
        }
    }

    #[tokio::test]
    async fn run_daily_leaves_when_how_and_what_it_did_in_meta() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (deps, _) = fake_deps(vec![strong_job()]);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).expect("the run was recorded");
        assert!(
            at >= open_now() && at < open_now() + TimeDelta::minutes(1),
            "the end of the run, {at}"
        );
        assert_eq!(result, "ok");
        assert_eq!(
            steps,
            serde_json::json!({
                "discover": {"ran": true, "new": 1},
                "prepare": {"ran": true, "prepared": 1},
                "submit": {"ran": true, "submitted": 0, "needs_gonza": 0},
                "dossiers": {"ran": true, "written": 0},
            }),
            "{out}"
        );

        // The panel reads back exactly what the run wrote, key for key.
        let panel = Panel::open(Paths::new(root.path()), Box::new(open_now)).unwrap();
        let controls = crate::panel::snapshot::build_snapshot(&panel)
            .map(|snapshot| serde_json::to_value(snapshot.controls).unwrap())
            .unwrap();
        assert_eq!(controls["last_run_result"], "ok");
        assert_eq!(controls["last_run_steps"], steps);
        assert_eq!(
            controls["last_run_at"].as_str(),
            Some(
                at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                    .as_str()
            )
        );
        assert_eq!(controls["engine"], "recent");
    }

    #[tokio::test]
    async fn run_daily_counts_only_what_this_run_moved() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Two applications already prepared by earlier runs, at the very same instant on purpose.
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "8".into()));
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "9".into()));
        let (deps, _) = fake_deps(vec![]);

        cli(root.path(), open_now(), deps, &["run-daily"]).await;

        let (_, _, steps) = last_run(root.path()).unwrap();
        assert_eq!(
            steps["prepare"],
            serde_json::json!({"ran": true, "prepared": 0})
        );
    }

    #[tokio::test]
    async fn the_next_hourly_run_notes_the_discovery_it_skipped_and_replaces_the_record() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let first = open_now();
        let second = first + TimeDelta::hours(1);
        cli(root.path(), first, fake_deps(vec![]).0, &["run-daily"]).await;
        let (before, _, _) = last_run(root.path()).unwrap();

        let (code, out) = cli(root.path(), second, fake_deps(vec![]).0, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).unwrap();
        assert!(at > before, "the record is of the latest run");
        assert_eq!(result, "ok");
        assert_eq!(
            steps["discover"],
            serde_json::json!({"ran": false, "skipped": "cooldown"})
        );
    }

    #[tokio::test]
    async fn a_stop_created_mid_run_is_recorded_as_stopped_with_the_steps_it_cost() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(StopWhileFetching(root.path().join("STOP")))]);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, EXIT_STOP, "{out}");
        let (_, result, steps) = last_run(root.path()).expect("a stopped run is recorded too");
        assert_eq!(result, "stopped");
        assert_eq!(steps["discover"]["ran"], serde_json::json!(true));
        for step in ["prepare", "submit", "dossiers"] {
            assert_eq!(
                steps[step],
                serde_json::json!({"ran": false, "skipped": "stop"}),
                "{step}"
            );
        }
    }

    #[tokio::test]
    async fn a_window_that_closes_mid_run_is_recorded_as_window_closed() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Same edge as `run_daily_stops_when_the_window_closes_during_the_run`.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 59).unwrap() + TimeDelta::milliseconds(600);
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(SlowSource(
            std::time::Duration::from_millis(1200),
        ))]);

        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).unwrap();
        assert_eq!(result, "window_closed");
        assert!(
            at >= start + TimeDelta::seconds(1),
            "the end, not the start: {at}"
        );
        assert_eq!(
            steps["prepare"],
            serde_json::json!({"ran": false, "skipped": "window_closed"})
        );
    }

    #[tokio::test]
    async fn a_failed_step_is_recorded_as_one_line_of_error() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("criteria.toml"),
            "this is = not [valid",
        )
        .unwrap();

        let (code, _) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR);
        let (_, result, steps) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: ") && result.contains("criteria") && !result.contains('\n'),
            "{result}"
        );
        assert!(
            result.chars().count() <= 307,
            "short enough to show: {result}"
        );
        assert_eq!(
            steps["discover"],
            serde_json::json!({"ran": false, "skipped": "error"})
        );
    }

    #[tokio::test]
    async fn a_cv_that_is_not_ready_is_an_error_that_skips_the_rest() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);

        let (code, _) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_CV_NOT_READY);
        let (_, result, steps) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: prepare terminó con código 2"),
            "{result}"
        );
        assert_eq!(steps["prepare"]["code"], serde_json::json!(2));
        assert_eq!(
            steps["submit"],
            serde_json::json!({"ran": false, "skipped": "after_error"})
        );
    }

    #[tokio::test]
    async fn a_login_that_is_not_private_is_recorded_as_an_error_of_the_run() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![]).refusing_account("la cuenta privada de jobbot no tiene sesión"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, _) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR);
        let (_, result, _) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: Claude no se usó") && result.contains("no tiene sesión"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn a_run_turned_away_at_the_door_leaves_the_last_real_run_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        let before = last_run(root.path()).unwrap();

        // The hourly task fires during working hours, and again with a STOP: both exit at once.
        assert_eq!(plain(root.path(), busy_now(), &["run-daily"]).await.0, 4);
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(plain(root.path(), open_now(), &["run-daily"]).await.0, 3);

        assert_eq!(
            last_run(root.path()).unwrap(),
            before,
            "nothing ran, so nothing is stamped (and the task's log stays empty)"
        );
    }

    #[tokio::test]
    async fn status_says_nothing_has_run_yet_and_the_login_is_unchecked() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["status"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Última corrida: ninguna todavía"), "{out}");
        assert!(out.contains("Cuenta de Claude: sin verificar"), "{out}");
    }

    #[tokio::test]
    async fn status_shows_the_last_run_its_steps_and_the_verified_account() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        cli(
            root.path(),
            open_now(),
            fake_deps(vec![strong_job()]).0,
            &["run-daily"],
        )
        .await;
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        cli(root.path(), open_now(), deps, &["claude-check"]).await;

        // Two hours later (and a little: the run ended a moment after it began), in Santiago's evening.
        let later = open_now() + TimeDelta::hours(2) + TimeDelta::seconds(30);
        let (code, out) = plain(root.path(), later, &["status"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Última corrida: 2026-10-14 19:00 (hora de Santiago, hace 2 h): ok"),
            "{out}"
        );
        assert!(
            out.contains("descubrir: 1 nuevas")
                && out.contains("preparar: 1 preparadas")
                && out.contains("enviar: 0 enviadas, 0 para ti")
                && out.contains("dossiers: 0 escritos"),
            "{out}"
        );
        assert!(
            out.contains(
                "Cuenta de Claude: verificada 2026-10-14 19:00 (hace 2 h) · método claude.ai"
            ) && out.contains("suscripción max · correo t***@example.com"),
            "{out}"
        );
        // The counts per state are still there.
        assert!(out.contains("prepared"), "{out}");
    }

    #[tokio::test]
    async fn status_says_why_the_last_login_check_failed_and_that_stop_is_on() {
        let root = temp_root();
        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        cli(
            root.path(),
            open_now(),
            with_claude(fake_deps(vec![]).0, &refused),
            &["claude-check"],
        )
        .await;
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (_, out) = plain(root.path(), open_now(), &["status"]).await;

        assert!(
            out.contains("Cuenta de Claude: la última verificación falló")
                && out.contains("cuenta equivocada"),
            "{out}"
        );
        assert!(out.contains("STOP activo"), "{out}");
    }

    #[tokio::test]
    async fn status_shows_an_error_run_and_a_skipped_step_in_plain_spanish() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);
        plain(root.path(), open_now(), &["run-daily"]).await;

        let (_, out) = plain(root.path(), open_now(), &["status"]).await;

        assert!(
            out.contains("hace un momento): error: prepare terminó con código 2"),
            "{out}"
        );
        assert!(out.contains("enviar: omitido (tras un error)"), "{out}");
    }
}
