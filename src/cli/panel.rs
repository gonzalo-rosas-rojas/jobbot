//! The `panel` command: serves the local panel on this PC and opens it in the browser.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};

use super::{EXIT_ERROR, PanelArgs};
use crate::panel::{scheduler::WindowsTasks, server::PanelServer};
use crate::paths::Paths;

/// The port `jobbot panel` asks for first (see `PanelArgs::port`).
const PREFERRED_PANEL_PORT: u16 = 47213;

/// Serves the local panel until the process is stopped (Ctrl+C). Like `prep` and `approve`, it
/// ignores STOP and the window: it only reads and writes the tracker and the prep store, on this
/// PC, and starts no discovery, browser or Claude. The clock is the start time plus the time
/// elapsed since, which keeps `main` the only place that reads the real one. The one thing it
/// asks of Windows is, read-only and in the background, whether the scheduled task is registered
/// and when it fires next (`panel::scheduler`).
pub(super) async fn cmd_panel(
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
        crate::panel::open(paths.clone(), clock)
            .map(|panel| panel.with_task_query(WindowsTasks::daily()))
    };
    let server = match args.port {
        Some(port) => PanelServer::start(open_panel()?, port)?,
        // A port that stays the same keeps the browser's saved settings (they belong to the origin, port included).
        None => match PanelServer::start(open_panel()?, PREFERRED_PANEL_PORT) {
            Ok(server) => server,
            Err(err) => {
                // Any failure of the preferred port (busy, not allowed) falls back to a free one;
                // the reason is kept for whoever asks why the port changed.
                tracing::warn!(
                    port = PREFERRED_PANEL_PORT,
                    error = %err,
                    "the preferred panel port is not available; using a free one"
                );
                PanelServer::start(open_panel()?, 0)?
            }
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
    // `wait` only returns when the serving loop ended on its own (Ctrl+C kills the process before
    // this line, and nothing here calls `shutdown`): that is a server that died, not a close.
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
    use crate::cli::test_support::{open_now, plain, temp_root};

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
}
