mod api;
mod app;
mod config;
mod import;
mod setup;
mod theme;
mod ui;

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::api::{Client, Failure, Project, Settings};
use crate::app::{App, AppOptions, Intent, Motion, Source};
use crate::config::{Config, ProjectConfig};
use crate::import::{Action as ImportAction, Import};
use crate::setup::{Action, Setup};

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

    let saved = Config::load();
    let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
    let cli_project = cli_project(&args)?;

    if args.demo {
        let sources = vec![
            Source::demo("MagicWeather", "proj_demo_1"),
            Source::demo("Notes", "proj_demo_2"),
            Source::demo("Habits", "proj_demo_3"),
            Source::demo("Focus", "proj_demo_4"),
            Source::demo("Budget", "proj_demo_5"),
            Source::demo("Recipes", "proj_demo_6"),
            Source::demo("Orbit", "proj_demo_7"),
            Source::demo("Ledger", "proj_demo_8"),
            Source::demo("Sketch", "proj_demo_9"),
            Source::demo("Pulse", "proj_demo_10"),
            Source::demo("Garden", "proj_demo_11"),
            Source::demo("Atlas", "proj_demo_12"),
            Source::demo("Ember", "proj_demo_13"),
            Source::demo("Compass", "proj_demo_14"),
            Source::demo("Vault", "proj_demo_15"),
            Source::demo("Lumen", "proj_demo_16"),
            Source::demo("Beacon", "proj_demo_17"),
        ];
        let app = build_app(sources, &saved, &currency, &args);
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

    let mut scope_id: Option<String> = None;
    loop {
        let saved = Config::load();
        let currency = resolve_currency(args.currency.as_deref(), saved.currency.as_deref());
        let sources = projects
            .iter()
            .map(|project| source_for(project, &args.base_url, &currency))
            .collect::<Vec<_>>();
        let mut app = build_app(sources, &saved, &currency, &args);
        if let Some(id) = &scope_id {
            app.set_scope_by_id(id);
        }
        let control = run_dashboard_once(&mut app)?;
        scope_id = app.scope_id().map(str::to_string);
        match control {
            Control::Quit => return Ok(ExitCode::SUCCESS),
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
    })
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

fn source_for(project: &ProjectConfig, base_url: &str, currency: &str) -> Source {
    let settings = Settings::new(
        base_url,
        project.api_key.clone(),
        project.id.clone(),
        currency,
    );
    Source::live(project.label(), project.id.clone(), Client::new(settings))
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
    let projects = import_loop(&mut terminal, &mut state, base_url, currency);
    ratatui::restore();
    projects
}

fn import_loop(
    terminal: &mut DefaultTerminal,
    state: &mut Import,
    base_url: &str,
    currency: &str,
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
                    let outcomes = import::validate(entries, base_url, currency);
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

    let runtime = runtime()?;
    let mut failures = 0;
    for project in &projects {
        let client = Client::new(Settings::new(
            args.base_url.clone(),
            project.api_key.clone(),
            project.id.clone(),
            &currency,
        ));
        println!(
            "Checking {} ({}) with key {} …",
            project.label(),
            project.id,
            client.settings().redacted_key()
        );
        let overview = runtime.block_on(client.fetch_overview());
        let charts = runtime.block_on(client.fetch_range("trials", 7, false));
        let mut ok = true;
        let mut printed = false;
        match &overview {
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
        match &charts {
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
