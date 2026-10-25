mod activity;
mod api;
mod app;
mod card;
mod chime;
mod config;
mod countries;
mod import;
mod lights;
mod moments;
mod notify;
mod service;
mod setup;
#[cfg(test)]
mod snapshot;
mod theme;
mod ui;
mod watch;

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use tokio::task::JoinSet;

use crate::api::{Client, Failure, Project, Settings};
use crate::app::{App, AppOptions, Intent, Motion, Source};
use crate::config::{Config, ProjectConfig};
use crate::import::{Action as ImportAction, Import};
use crate::setup::{Action, Setup};

const DEMO_ENOUGH: f64 = 3000.0;

struct CheckOutcome {
    order: usize,
    label: String,
    id: String,
    key: String,
    overview: Result<api::Overview, Failure>,
    charts: Result<Vec<f64>, Failure>,
}

#[derive(Parser)]
#[command(version, about = "RevenueCat overview in the terminal")]
struct Args {
    /// RevenueCat project ID (for example proj1a2b3c4d)
    #[arg(long, env = "RC_PROJECT_ID")]
    project_id: Option<String>,

    /// RevenueCat v2 secret API key (sk_...)
    #[arg(long, env = "RC_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    /// Currency for monetary metrics
    #[arg(long, env = "RC_CURRENCY")]
    currency: Option<String>,

    /// Seconds between automatic refreshes
    #[arg(long, default_value_t = 300)]
    interval: u64,

    /// RevenueCat v2 API base URL
    #[arg(long, default_value = "https://api.revenuecat.com/v2")]
    base_url: String,

    /// Generate demo data instead of calling the API
    #[arg(long)]
    demo: bool,

    /// Validate every configured project and exit
    #[arg(long)]
    check: bool,

    /// Edit or re-run the setup wizard even when credentials are already saved
    #[arg(long)]
    setup: bool,

    /// Add another project without touching the saved ones
    #[arg(long)]
    add_project: bool,

    /// Bulk-add projects by pasting one secret key per line
    #[arg(long)]
    import: bool,

    /// Open the dashboard on one moment (the notification's "Open rc-tui" uses this)
    #[arg(long, value_name = "ID")]
    moment: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Watch in the background and turn records, milestones and firsts into moments
    Watch {
        /// Install and start the watcher as a login service (systemd user unit or launchd agent)
        #[arg(long)]
        install: bool,
        /// Stop and remove the login service
        #[arg(long)]
        uninstall: bool,
        /// Learn and print what would fire once, without saving or notifying
        #[arg(long)]
        dry_run: bool,
    },
    /// List your moments, render the yearbook, or send a sample through every channel
    Moments {
        /// Send a sample moment through every channel (notification, chime, lights, command)
        #[arg(long)]
        test: bool,
        /// Render the year's moments as one poster (this year unless one is given)
        #[arg(long, value_name = "YEAR", num_args = 0..=1, default_missing_value = "")]
        yearbook: Option<String>,
        /// Render a moment's card again, for example after adding a note
        #[arg(long, value_name = "ID")]
        card: Option<String>,
    },
}

enum Control {
    Quit,
    Setup,
    AddProject,
    Import,
    EditProject(usize),
    RemoveProject(usize),
}

fn main() -> color_eyre::Result<ExitCode> {
    color_eyre::install()?;
    let args = Args::parse();

    if args.check {
        return run_check(&args);
    }
    if let Some(command) = &args.command {
        return run_command(command, &args);
    }

    let saved = Config::load();
    theme::init(saved.theme.as_deref());
    let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
    let cli_project = cli_project(&args)?;

    if args.demo {
        let sources = api::DEMO_PROJECTS
            .iter()
            .enumerate()
            .map(|(index, name)| Source::demo(*name, format!("proj_demo_{}", index + 1)))
            .collect();
        let mut app = build_app(sources, &saved, &currency, &args);
        app.enough.get_or_insert(DEMO_ENOUGH);
        return run_dashboard(app);
    }

    let mut projects = match &cli_project {
        Some(project) => vec![project.clone()],
        None => saved.projects.clone(),
    };

    if args.import {
        if !std::io::stdin().is_terminal() {
            print_setup_help();
            return Ok(ExitCode::FAILURE);
        }
        let added = run_import(&args.base_url, &currency)?;
        if added.is_empty() && projects.is_empty() {
            eprintln!("Import cancelled — nothing was saved. Run rc-tui again to try again.");
            return Ok(ExitCode::FAILURE);
        }
        if !added.is_empty() {
            projects = persist(added, &currency);
        }
    } else if args.add_project || args.setup || projects.is_empty() {
        if !std::io::stdin().is_terminal() {
            print_setup_help();
            return Ok(ExitCode::FAILURE);
        }
        let prefill = if args.add_project {
            None
        } else {
            projects.first()
        };
        let (added, chosen_currency) = run_setup(&args.base_url, prefill, &currency)?;
        if added.is_empty() && projects.is_empty() {
            eprintln!("Setup cancelled — nothing was saved. Run rc-tui again to try again.");
            return Ok(ExitCode::FAILURE);
        }
        projects = persist(added, &chosen_currency);
    }

    let mut scope_id: Option<String> = saved.last_project.clone();
    loop {
        let saved = Config::load();
        let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
        let http = api::http_client();
        let sources = projects
            .iter()
            .map(|project| source_for(project, &args.base_url, &currency, &http))
            .collect::<Vec<_>>();
        let mut app = build_app(sources, &saved, &currency, &args);
        if let Some(id) = &scope_id {
            app.set_scope_by_id(id);
        }
        if let Some(id) = args.moment.as_deref() {
            app.open_moments(Some(id));
        }
        let control = run_dashboard_once(&mut app)?;
        scope_id = app.scope_id().map(str::to_string);
        match control {
            Control::Quit => {
                if !app.demo {
                    save_last_scope(scope_id.as_deref());
                }
                return Ok(ExitCode::SUCCESS);
            }
            Control::Import => {
                if !std::io::stdin().is_terminal() {
                    return Ok(ExitCode::FAILURE);
                }
                let added = run_import(&args.base_url, &currency)?;
                if added.is_empty() {
                    continue;
                }
                projects = persist(added, &currency);
            }
            Control::EditProject(index) => {
                if !std::io::stdin().is_terminal() {
                    return Ok(ExitCode::FAILURE);
                }
                let Some(project) = app.views.get(index) else {
                    continue;
                };
                let id = project.id.clone();
                let prefill = Config::load()
                    .projects
                    .into_iter()
                    .find(|project| project.id == id);
                let (added, chosen_currency) =
                    run_setup(&args.base_url, prefill.as_ref(), &currency)?;
                if added.is_empty() {
                    continue;
                }
                projects = persist(added, &chosen_currency);
                scope_id = Some(id);
            }
            Control::RemoveProject(index) => {
                let Some(project) = app.views.get(index) else {
                    continue;
                };
                let id = project.id.clone();
                let mut config = Config::load();
                config.remove_project(&id);
                if let Err(err) = config.save() {
                    eprintln!("rc-tui: could not save the config file: {err}");
                    continue;
                }
                projects = config.projects;
                scope_id = None;
                if projects.is_empty() {
                    eprintln!("No projects left to watch.");
                    return Ok(ExitCode::SUCCESS);
                }
            }
            control => {
                if !std::io::stdin().is_terminal() {
                    return Ok(ExitCode::FAILURE);
                }
                let prefill = match control {
                    Control::AddProject => None,
                    _ => projects.first(),
                };
                let (added, chosen_currency) = run_setup(&args.base_url, prefill, &currency)?;
                if added.is_empty() {
                    continue;
                }
                projects = persist(added, &chosen_currency);
            }
        }
    }
}

fn build_app(sources: Vec<Source>, config: &Config, currency: &str, args: &Args) -> App {
    App::new(AppOptions {
        sources,
        currency: currency.to_string(),
        interval: refresh_interval(args.interval),
        chart_ttl: Duration::from_secs(config.chart_minutes.unwrap_or(15).max(1) * 60),
        motion: Motion::parse(config.motion.as_deref()),
        wall: config.wall.unwrap_or(false),
        pins: config.pins.clone(),
        ui_scale: config.ui_scale.unwrap_or(0).min(3),
        ui_page: config.ui_page.unwrap_or(0),
        feed_path: config.feed_path.as_ref().map(std::path::PathBuf::from),
        feed_seen_at: config.feed_seen_at,
        enough: config.enough.map(|amount| amount as f64),
    })
}

fn run_command(command: &Command, args: &Args) -> color_eyre::Result<ExitCode> {
    let saved = Config::load();
    let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
    match command {
        Command::Watch { install: true, .. } => {
            let exe = std::env::current_exe()?;
            match service::install(&exe) {
                Ok(lines) => {
                    for line in lines {
                        println!("{line}");
                    }
                    println!(
                        "rc-tui watch is running. Logs: journalctl --user -u {} -f",
                        service::UNIT
                    );
                    Ok(ExitCode::SUCCESS)
                }
                Err(err) => {
                    eprintln!("rc-tui: {err}");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Command::Watch {
            uninstall: true, ..
        } => match service::uninstall() {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                Ok(ExitCode::SUCCESS)
            }
            Err(err) => {
                eprintln!("rc-tui: {err}");
                Ok(ExitCode::FAILURE)
            }
        },
        Command::Watch { dry_run, .. } => watch::run(watch::Options {
            base_url: args.base_url.clone(),
            currency,
            dry_run: *dry_run,
        }),
        Command::Moments { test: true, .. } => watch::test(&currency),
        Command::Moments {
            yearbook: Some(year),
            ..
        } => {
            let year = if year.is_empty() {
                chrono::Datelike::year(&chrono::Local::now())
            } else {
                year.parse()?
            };
            let path = card::yearbook(
                &moments::load(&moments::log_path()),
                year,
                &cards_dir(&saved),
            )?;
            println!("{}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Moments { card: Some(id), .. } => {
            let all = moments::load(&moments::log_path());
            let Some(moment) = all
                .iter()
                .rev()
                .find(|moment| moment.id == *id || moment.id.starts_with(id.as_str()))
            else {
                eprintln!("rc-tui: no moment with id {id}. `rc-tui moments` lists them.");
                return Ok(ExitCode::FAILURE);
            };
            let path = card::render(moment, &cards_dir(&saved))?;
            println!("{}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Moments { .. } => {
            print_moments();
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn cards_dir(config: &Config) -> std::path::PathBuf {
    match config.moments.cards.as_deref() {
        Some(dir) => match (dir.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
            _ => std::path::PathBuf::from(dir),
        },
        None => config::pictures_dir().join("rc-tui"),
    }
}

/// Prints every moment worth keeping, newest first, with its note and card, then whether the watcher runs.
fn print_moments() {
    let all = moments::load(&moments::log_path());
    let kept: Vec<&moments::Moment> = all
        .iter()
        .rev()
        .filter(|moment| moment.kind.keeps() && !moment.test)
        .collect();
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, text: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    };
    if kept.is_empty() {
        if matches!(
            service::status().as_deref(),
            Some("active") | Some("loaded")
        ) {
            println!("No moments yet. The watcher is running; they arrive when something happens.");
            return;
        }
        println!("No moments yet. They arrive while `rc-tui watch` runs:");
        println!("  rc-tui watch --install    start it now and at every login");
        println!("  rc-tui moments --test     see what one looks like");
        return;
    }
    for moment in kept {
        let date = moment
            .local_time()
            .map(|time| time.format("%-d %b %Y").to_string())
            .unwrap_or_default();
        let (red, green, blue) = card::tone_rgb(moment.kind.tone());
        let accent = format!("1;38;2;{red};{green};{blue}");
        println!(
            "{}  {} {}  {}",
            paint("2", &format!("{date:>11}")),
            paint(&accent, moment.kind.glyph()),
            paint("1", &moment.title),
            paint(&accent, &moment.value)
        );
        println!("{}{}", " ".repeat(15), moment.line);
        if let Some(note) = &moment.note {
            println!("{}{}", " ".repeat(15), paint("3", &format!("“{note}”")));
        }
        if let Some(card) = &moment.card {
            println!("{}{}", " ".repeat(15), paint("2", card));
        }
        println!("{}{}", " ".repeat(15), paint("2", &moment.id));
        println!();
    }
    match service::status().as_deref() {
        Some("active") | Some("loaded") => println!("The watcher is running."),
        Some(state) => println!("The watcher is {state}. Start it with: rc-tui watch --install"),
        None => {}
    }
}

fn save_last_scope(scope: Option<&str>) {
    let mut config = Config::load();
    config.last_project = scope.map(str::to_string);
    if let Err(err) = config.save() {
        eprintln!("rc-tui: could not save the last project: {err}");
    }
}

fn persist(added: Vec<ProjectConfig>, currency: &str) -> Vec<ProjectConfig> {
    if added.is_empty() {
        return Config::load().projects;
    }
    let mut config = Config::load();
    config.currency = Some(currency.to_string());
    for project in &added {
        config.upsert(project.clone());
    }
    match config.save() {
        Ok(path) => eprintln!(
            "Saved {} project(s) to {} — delete an entry there to stop monitoring it.",
            added.len(),
            path.display()
        ),
        Err(err) => eprintln!("rc-tui: could not save the config file: {err}"),
    }
    config.projects
}

fn source_for(
    project: &ProjectConfig,
    base_url: &str,
    currency: &str,
    http: &reqwest::Client,
) -> Source {
    let settings = Settings::new(
        base_url,
        project.api_key.clone(),
        project.id.clone(),
        currency,
    );
    Source::live(
        project.label(),
        project.id.clone(),
        Client::with_http(settings, http.clone()),
    )
}

fn run_dashboard(mut app: App) -> color_eyre::Result<ExitCode> {
    let _ = run_dashboard_once(&mut app)?;
    Ok(ExitCode::SUCCESS)
}

fn run_dashboard_once(app: &mut App) -> color_eyre::Result<Control> {
    let mut terminal = ratatui::try_init()?;
    let outcome = dashboard_loop(&mut terminal, app);
    ratatui::restore();
    outcome
}

fn dashboard_loop(terminal: &mut DefaultTerminal, app: &mut App) -> color_eyre::Result<Control> {
    loop {
        terminal.draw(|frame| ui::draw(app, frame))?;
        app.tick();
        if event::poll(app.poll_timeout())? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match app.handle_key(key) {
                    Intent::None => {}
                    Intent::Quit => return Ok(Control::Quit),
                    Intent::Setup => return Ok(Control::Setup),
                    Intent::AddProject => return Ok(Control::AddProject),
                    Intent::Import => return Ok(Control::Import),
                    Intent::EditProject(index) => return Ok(Control::EditProject(index)),
                    Intent::RemoveProject(index) => return Ok(Control::RemoveProject(index)),
                },
                _ => {}
            }
        }
    }
}

fn run_setup(
    base_url: &str,
    prefill: Option<&ProjectConfig>,
    currency: &str,
) -> color_eyre::Result<(Vec<ProjectConfig>, String)> {
    let mut terminal = ratatui::try_init()?;
    let mut wizard = match prefill {
        Some(project) => Setup::new(
            base_url,
            project.api_key.clone(),
            project.id.clone(),
            currency,
        )
        .with_name(project.name.clone())
        .edit_key(),
        None => Setup::new(base_url, "", "", currency),
    };
    let projects = wizard_loop(&mut terminal, &mut wizard);
    ratatui::restore();
    let projects = projects?;
    let currency = wizard.currency_value();
    Ok((projects, currency))
}

fn run_import(base_url: &str, currency: &str) -> color_eyre::Result<Vec<ProjectConfig>> {
    let mut terminal = ratatui::try_init()?;
    let mut state = Import::new();
    let http = api::http_client();
    let projects = import_loop(&mut terminal, &mut state, base_url, currency, &http);
    ratatui::restore();
    projects
}

fn import_loop(
    terminal: &mut DefaultTerminal,
    state: &mut Import,
    base_url: &str,
    currency: &str,
    http: &reqwest::Client,
) -> color_eyre::Result<Vec<ProjectConfig>> {
    loop {
        terminal.draw(|frame| ui::draw_import(state, frame))?;
        if !event::poll(Duration::from_millis(33))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match state.handle_key(key) {
                ImportAction::None => {}
                ImportAction::Cancel => return Ok(Vec::new()),
                ImportAction::Save => return Ok(state.valid_projects()),
                ImportAction::Validate(entries) => {
                    terminal.draw(|frame| ui::draw_import(state, frame))?;
                    let outcomes = import::validate(entries, base_url, currency, http);
                    state.finish(outcomes);
                }
            },
            _ => {}
        }
    }
}

fn wizard_loop(
    terminal: &mut DefaultTerminal,
    wizard: &mut Setup,
) -> color_eyre::Result<Vec<ProjectConfig>> {
    let mut completed = Vec::new();
    loop {
        terminal.draw(|frame| ui::draw_setup(wizard, frame))?;
        if !event::poll(Duration::from_millis(33))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match wizard.handle_key(key) {
                Action::None => {}
                Action::Quit | Action::Finish => return Ok(completed),
                Action::ListProjects { base_url, api_key } => {
                    terminal.draw(|frame| ui::draw_setup(wizard, frame))?;
                    wizard.projects_result(list_projects_blocking(&base_url, &api_key));
                }
                Action::Start(settings) => {
                    terminal.draw(|frame| ui::draw_setup(wizard, frame))?;
                    match check_settings(&settings) {
                        Ok(()) => {
                            completed.push(wizard.take_project());
                            wizard.connected();
                        }
                        Err(failure) => wizard.validation_failed(failure),
                    }
                }
            },
            _ => {}
        }
    }
}

fn run_check(args: &Args) -> color_eyre::Result<ExitCode> {
    let saved = Config::load();
    let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
    let cli_project = cli_project(args)?;

    if args.demo {
        let snapshot = api::demo_snapshot(&currency, 1);
        println!("Demo mode: no API calls were made.");
        print_snapshot(&snapshot);
        return Ok(ExitCode::SUCCESS);
    }

    let projects = match cli_project {
        Some(project) => vec![project],
        None => saved.projects,
    };
    if projects.is_empty() {
        print_setup_help();
        return Ok(ExitCode::FAILURE);
    }

    println!("Checking {} project(s) in parallel…\n", projects.len());
    let runtime = runtime()?;
    let http = api::http_client();
    let mut outcomes = runtime.block_on(async {
        let mut set = JoinSet::new();
        for (order, project) in projects.iter().cloned().enumerate() {
            let base_url = args.base_url.clone();
            let currency = currency.clone();
            let http = http.clone();
            set.spawn(async move {
                let client = Client::with_http(
                    Settings::new(
                        base_url,
                        project.api_key.clone(),
                        project.id.clone(),
                        &currency,
                    ),
                    http,
                );
                let key = client.settings().redacted_key();
                let label = project.label().to_string();
                let id = project.id;
                let overview = client.fetch_overview().await;
                let charts = client.fetch_range("trials", 7, false, 0).await;
                CheckOutcome {
                    order,
                    label,
                    id,
                    key,
                    overview,
                    charts,
                }
            });
        }
        let mut outcomes = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => {
                    let failure = Failure::local(format!("task failed: {err}"));
                    outcomes.push(CheckOutcome {
                        order: usize::MAX,
                        label: "worker".to_string(),
                        id: String::new(),
                        key: String::new(),
                        overview: Err(failure.clone()),
                        charts: Err(failure),
                    });
                }
            }
        }
        outcomes.sort_by_key(|outcome| outcome.order);
        outcomes
    });

    let mut failures = 0;
    for outcome in outcomes.drain(..) {
        println!(
            "Checking {} ({}) with key {} …",
            outcome.label, outcome.id, outcome.key
        );
        let mut ok = true;
        let mut printed = false;
        match &outcome.overview {
            Ok(overview) => {
                println!("  metrics ✓ {} metrics", overview.metrics.len());
                for metric in &overview.metrics {
                    println!(
                        "    {:<22} {}",
                        metric.name,
                        ui::format_metric(metric.value, &metric.unit, &overview.currency)
                    );
                }
            }
            Err(failure) => {
                ok = false;
                printed = true;
                println!("  metrics ✗ {}", failure.headline());
                print_failure_detail(failure);
            }
        }
        match &outcome.charts {
            Ok(_) => println!("  charts  ✓"),
            Err(failure) => {
                ok = false;
                println!("  charts  ✗ {}", failure.headline());
                if printed {
                    if let Some(detail) = failure.detail() {
                        println!("    {detail}");
                    }
                } else {
                    print_failure_detail(failure);
                }
            }
        }
        if !ok {
            failures += 1;
        }
        println!();
    }

    if failures == 0 {
        println!(
            "All {} project(s) have metrics and charts. Run `rc-tui` to open the dashboard.",
            projects.len()
        );
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn print_snapshot(snapshot: &api::Snapshot) {
    for metric in &snapshot.metrics {
        println!(
            "  {:<22} {}",
            metric.name,
            ui::format_metric(metric.value, &metric.unit, &snapshot.currency)
        );
    }
}

fn print_failure_detail(failure: &Failure) {
    if let Some(detail) = failure.detail() {
        println!("    {detail}");
    }
    for hint in failure.hints().iter().take(2) {
        println!("    → {hint}");
    }
    if let Some(doc_url) = &failure.doc_url {
        println!("    docs: {doc_url}");
    }
}

fn print_setup_help() {
    println!("rc-tui needs a RevenueCat v2 secret API key per project.");
    println!("There is no account-wide API key: secret keys are project-scoped.");
    println!();
    println!("  1. RevenueCat dashboard → your project → API keys → “+ New secret API key”.");
    println!(
        "  2. Grant it Charts & Metrics: Read  ({} and {}).",
        api::SCOPE_OVERVIEW,
        api::SCOPE_CHARTS
    );
    println!(
        "     Add Project Configuration: Read ({}) so rc-tui can pick the project.",
        api::SCOPE_PROJECTS
    );
    println!(
        "  3. Run `rc-tui` in a terminal to paste the key; answer “y” to add the next project."
    );
    println!();
    println!(
        "Saves one [[projects]] entry per project in {} (mode 600).",
        Config::path().display()
    );
    println!("Docs: {}", api::DOCS_API_KEYS);
}

fn cli_project(args: &Args) -> color_eyre::Result<Option<ProjectConfig>> {
    let api_key = args
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let project_id = args
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (api_key, project_id) {
        (Some(api_key), Some(project_id)) => {
            Ok(Some(ProjectConfig::new(project_id, api_key, None)))
        }
        (None, None) => Ok(None),
        _ => Err(color_eyre::eyre::eyre!(
            "set both --api-key and --project-id, or neither (flags override the config file)"
        )),
    }
}

fn list_projects_blocking(base_url: &str, api_key: &str) -> Result<Vec<Project>, Failure> {
    let client = Client::new(Settings::new(base_url, api_key, String::new(), "USD"));
    runtime()
        .map_err(|err| Failure::local(format!("could not start the network runtime: {err}")))?
        .block_on(client.list_projects())
}

fn check_settings(settings: &Settings) -> Result<(), Failure> {
    let client = Client::new(settings.clone());
    let report = runtime()
        .map_err(|err| Failure::local(format!("could not start the network runtime: {err}")))?
        .block_on(client.check_scopes());
    match report.failure() {
        Some(failure) => Err(failure.clone()),
        None => Ok(()),
    }
}

fn runtime() -> color_eyre::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

fn resolve_currency(cli: Option<&str>, saved: Option<&str>) -> String {
    cli.or(saved)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| "EUR".to_string())
}

fn refresh_interval(seconds: u64) -> Duration {
    Duration::from_secs(seconds.max(5))
}
