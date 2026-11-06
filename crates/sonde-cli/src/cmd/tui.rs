//! `sonde tui`: the terminal UI, in this one binary.
//!
//! The app's bundle is embedded at build time; it runs on Native Shell's
//! terminal backend (`nsh-platform-tui`) with sonde's Rust module linked
//! in. The host loop is sonde's own (rather than `nsh-tui-host`'s, whose
//! entry point parses the process's arguments and owns one view): it adds
//! OSC 52 copying, and shows a plugin's view — a signed package verified
//! against the trust roots — in a second engine of its own, sandboxed,
//! answering only its `sonde.read.*` host calls. F12 closes a plugin.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use base64::Engine as _;
use clap::Args;
use crossterm::event::{self, Event, KeyCode};
use crossterm::{execute, terminal};
use nsh_platform_tui::{
    AppOptions, AppView, Bundle, ColorDepth, Limits, RunEvent, TerminalCapabilities,
};
use ratatui_core::terminal::Terminal;
use ratatui_crossterm::CrosstermBackend;
use sonde_nsh::plugins::{self, Context, Plugins};

use crate::Cli;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/bundle.rs"));
}

#[derive(Args)]
pub struct TuiArgs {
    /// Run this bundle (`nsh build`'s bundle.js) instead of the embedded one.
    #[arg(long)]
    bundle: Option<PathBuf>,
    /// Start with the built-in demo endpoint.
    #[arg(long)]
    demo: bool,
    /// Show the frame-rate line (C-h p toggles it).
    #[arg(long)]
    fps: bool,
    /// Leave the mouse to the terminal.
    #[arg(long)]
    no_mouse: bool,
    /// Diagnostics go here (default: the system's temporary directory).
    #[arg(long)]
    log_file: Option<PathBuf>,
}

/// The system clipboard through the terminal: OSC 52, over SSH too.
#[derive(Default)]
struct Osc52 {
    text: Mutex<Option<String>>,
    pending: Mutex<Option<String>>,
}

impl nsh_core::clipboard::ClipboardBackend for Osc52 {
    fn text(&self) -> Option<String> {
        self.text.lock().unwrap().clone()
    }
    fn set_text(&self, text: Option<String>) {
        *self.text.lock().unwrap() = text.clone();
        *self.pending.lock().unwrap() = text;
    }
}

impl Osc52 {
    fn flush(&self, out: &mut impl Write) -> io::Result<()> {
        if let Some(t) = self.pending.lock().unwrap().take() {
            write!(
                out,
                "\x1b]52;c;{}\x07",
                base64::engine::general_purpose::STANDARD.encode(t)
            )?;
            out.flush()?;
        }
        Ok(())
    }
}

fn capabilities() -> TerminalCapabilities {
    let var = |n: &str| std::env::var(n).ok().map(|v| v.to_ascii_lowercase());
    let mut c = TerminalCapabilities::default();
    c.colors = if var("NO_COLOR").is_some() {
        ColorDepth::Monochrome
    } else if matches!(
        var("COLORTERM").as_deref(),
        Some("truecolor") | Some("24bit")
    ) {
        ColorDepth::TrueColor
    } else if var("TERM").is_some_and(|t| t.contains("256color")) {
        ColorDepth::Ansi256
    } else {
        ColorDepth::Ansi16
    };
    // COLORFGBG ("15;0": light text on a dark background)
    if let Some(bg) =
        var("COLORFGBG").and_then(|v| v.rsplit(';').next().and_then(|b| b.parse::<u8>().ok()))
    {
        let light = bg == 7 || bg == 15;
        c.background = if light {
            nsh_core::platform::ColorScheme::Light
        } else {
            nsh_core::platform::ColorScheme::Dark
        };
        c.background_color = Some(if light { [250, 250, 250] } else { [16, 16, 18] });
    } else {
        c.background_color = Some([16, 16, 18]);
    }
    c
}

fn bundle(path: &Option<PathBuf>) -> io::Result<Bundle> {
    if let Some(p) = path {
        return Bundle::read(p);
    }
    let Some(js) = embedded::BUNDLE else {
        return Err(io::Error::other(
            "this build has no embedded app: build the app first (`nsh build app`), or pass --bundle",
        ));
    };
    let mut b = Bundle::source(js.to_string()).with_app_id(Some("dev.sonde.app"));
    if let Some(map) = embedded::SOURCE_MAP {
        b = b.with_source_map(map);
    }
    b.with_capabilities_json(embedded::CAPABILITIES)
        .map_err(io::Error::other)
}

struct Session {
    mouse: bool,
}

impl Session {
    fn enter(mouse: bool) -> io::Result<Session> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, terminal::EnterAlternateScreen, crossterm::cursor::Hide)?;
        let _ = execute!(out, event::EnableBracketedPaste, event::EnableFocusChange);
        if mouse {
            execute!(out, event::EnableMouseCapture)?;
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore(mouse);
            previous(info);
        }));
        Ok(Session { mouse })
    }
}

fn restore(mouse: bool) {
    let mut out = io::stdout();
    if mouse {
        let _ = execute!(out, event::DisableMouseCapture);
    }
    let _ = execute!(
        out,
        event::DisableBracketedPaste,
        event::DisableFocusChange,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}

impl Drop for Session {
    fn drop(&mut self) {
        restore(self.mouse);
    }
}

fn options(clipboard: &Arc<Osc52>, fps: bool) -> AppOptions {
    let mut o = AppOptions::default();
    o.limits = Limits::standalone();
    o.capabilities = capabilities();
    o.clipboard = Some(clipboard.clone());
    o.fps = fps;
    o.data_dir = nsh_engine::data_dir();
    o
}

/// The plugin view: a package's bundle in an engine of its own.
fn plugin_view(
    id: &str,
    ctx: Context,
    clipboard: &Arc<Osc52>,
    sonde: &Arc<sonde_core::Sonde>,
    rt: &tokio::runtime::Handle,
) -> Result<AppView, String> {
    let p = Plugins::open(sonde);
    let bytes = p.load(id)?;
    let trust = p.trust_store()?;
    let loaded = nsh_engine::verify_package(&trust, &bytes, &p.dir().join("assets"))
        .map_err(|e| format!("refused: {e}"))?;
    let mut o = options(clipboard, false);
    o.ui_sets = vec![nsh_base::registry()];
    o.limits = Limits::default();
    o.clipboard = None;
    o.data_dir = Some(p.dir().join("data").join(id));
    // Only the read-only calls, each answered on the runtime's threads.
    for name in plugins::HOST_CALLS {
        let (s, ctx, rt) = (sonde.clone(), ctx.clone(), rt.clone());
        o.host.handle(*name, move |call| {
            let (s, ctx) = (s.clone(), ctx.clone());
            let name = call.name().to_string();
            let data = call.data().clone();
            rt.spawn(async move {
                match plugins::host_call(&s, &ctx, &name, &data).await {
                    Ok(v) => call.resolve(v),
                    Err(e) => call.reject(e),
                }
            });
        });
    }
    let mut view = AppView::new(o);
    view.load(loaded.bundle).map_err(|e| e.to_string())?;
    Ok(view)
}

pub fn run(cli: &Cli, args: &TuiArgs) -> ExitCode {
    match run_inner(cli, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sonde tui: {e}");
            ExitCode::from(70)
        }
    }
}

fn run_inner(cli: &Cli, args: &TuiArgs) -> io::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let sonde = super::open(cli).map_err(io::Error::other)?;
    sonde_nsh::install(sonde.clone());
    if args.demo {
        rt.block_on(sonde_core::demo::install(&sonde))
            .map_err(io::Error::other)?;
    }
    let log = args.log_file.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("sonde-tui-{}.log", std::process::id()))
    });
    let clipboard = Arc::new(Osc52::default());
    let (open_tx, open_rx) = mpsc::channel::<(String, serde_json::Value)>();
    plugins::set_host(Arc::new(move |id, ctx| {
        let _ = open_tx.send((id, ctx));
    }));
    let mut o = options(&clipboard, args.fps);
    o.ui_sets = vec![nsh_base::registry(), sonde_nsh::registry()];
    let mut main = AppView::new(o);
    main.set_dev_log_path(log);
    let session = Session::enter(!args.no_mouse)?;
    let backend = CrosstermBackend::new(io::BufWriter::new(io::stdout()));
    let mut term = Terminal::new(main.measure_backend(backend))?;
    term.draw(|f| main.render(f, f.area()))?;
    main.load(bundle(&args.bundle)?)?;
    let mut plugin: Option<AppView> = None;
    let mut plugin_term: Option<Terminal<_>> = None;
    let mut redraw = true;
    const POLL: Duration = Duration::from_millis(16);
    loop {
        if let Ok((id, ctx)) = open_rx.try_recv() {
            match serde_json::from_value::<Context>(ctx)
                .map_err(|e| e.to_string())
                .and_then(|c| plugin_view(&id, c, &clipboard, &sonde, rt.handle()))
            {
                Ok(v) => {
                    let backend = CrosstermBackend::new(io::BufWriter::new(io::stdout()));
                    let mut t = Terminal::new(v.measure_backend(backend))?;
                    t.clear()?;
                    plugin_term = Some(t);
                    plugin = Some(v);
                    redraw = true;
                }
                Err(e) => log_line(&format!("plugin {id}: {e}")),
            }
        }
        if let (Some(v), Some(t)) = (plugin.as_mut(), plugin_term.as_mut()) {
            if redraw || v.next_frame_deadline().is_some_and(|d| d <= Instant::now()) {
                v.draw_terminal(t)?;
                redraw = false;
            }
            if v.exit_requested() {
                close_plugin(&mut plugin, &mut plugin_term, &mut term)?;
                redraw = true;
                continue;
            }
        } else {
            if redraw
                || main
                    .next_frame_deadline()
                    .is_some_and(|d| d <= Instant::now())
            {
                main.draw_terminal(&mut term)?;
                redraw = false;
            }
            if main.exit_requested() {
                break;
            }
        }
        clipboard.flush(&mut io::stdout())?;
        let active = plugin.as_ref().unwrap_or(&main);
        let wait = active.next_frame_deadline().map_or(POLL, |d| {
            d.saturating_duration_since(Instant::now()).min(POLL)
        });
        if event::poll(wait)? {
            let ev = event::read()?;
            if let Event::Key(k) = &ev
                && k.code == KeyCode::F(12)
                && plugin.is_some()
            {
                close_plugin(&mut plugin, &mut plugin_term, &mut term)?;
                redraw = true;
                continue;
            }
            match nsh_tui_host::convert(ev) {
                RunEvent::Input(input) => {
                    match plugin.as_mut() {
                        Some(v) => v.handle(input),
                        None => main.handle(input),
                    };
                }
                RunEvent::Redraw => redraw = true,
                RunEvent::Exit => break,
                _ => {}
            }
        }
    }
    if let Some(mut v) = plugin.take() {
        v.stop();
    }
    main.stop();
    drop(term);
    drop(session);
    Ok(())
}

fn close_plugin<B: ratatui_core::backend::Backend>(
    plugin: &mut Option<AppView>,
    plugin_term: &mut Option<Terminal<B>>,
    term: &mut Terminal<impl ratatui_core::backend::Backend>,
) -> io::Result<()> {
    if let Some(mut v) = plugin.take() {
        v.stop();
    }
    *plugin_term = None;
    term.clear().map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

fn log_line(msg: &str) {
    let path = std::env::temp_dir().join("sonde-tui-plugins.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{} {msg}", chrono::Utc::now().to_rfc3339());
    }
}
