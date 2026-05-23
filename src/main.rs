use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "org")]
#[command(about = "Inspect the active institution graph")]
struct Cli {
    /// Root orgmap.toml. When omitted, org walks upward, then checks ORGMAP_CONFIG and XDG config.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List configured institutions.
    List {
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
    },
    /// Current operational report for the active institution.
    Work {
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
    },
    /// Familiarization report: workspaces and project descriptions.
    Intro {
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
    },
    /// Orgmap project report grouped by workspace.
    Report {
        /// Optional project name filter.
        project: Option<String>,
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
    },
    /// Local git state report for org projects.
    Git {
        /// Optional project name filter.
        project: Option<String>,
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
    },
    /// Agent plugin carrier report.
    Plug {
        /// Verify GitHub manifest parity and carrier repo git state.
        #[arg(long)]
        upstream: bool,
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        command: Option<PlugCommand>,
    },
    /// Fuzzy-pick an org root, workgroup, or local project path for cd navigation.
    Fzf {
        /// Print all selectable rows without launching fzf.
        #[arg(long)]
        list: bool,
        /// Emit a shell-safe `cd <path>` command instead of only the path.
        #[arg(long)]
        cd: bool,
        /// Emit JSON instead of terminal text. With --list, emits every row.
        #[arg(long)]
        json: bool,
    },
    /// Resolve the nearest orgmap identity for a path.
    Identity {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Resolve the nearest full orgmap definition for a path.
    Definition {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Resolve every ancestor orgmap marker for a path.
    Stack {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Resolve the marker file watched for a path.
    Marker {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Anti-sloppy screen — scan every local project for secrets, personal
    /// path/identity leaks, and pre-publish sloppiness. Exits nonzero when
    /// any Critical or Warn finding lands.
    Screen {
        /// Optional project name filter — limit the scan to one project.
        project: Option<String>,
        /// Emit JSON instead of terminal text.
        #[arg(long)]
        json: bool,
        /// Skip the gitleaks shell-out even if the binary is on PATH.
        #[arg(long)]
        no_gitleaks: bool,
        /// Only show Critical (secret) findings; suppress pathleak + sloppy.
        #[arg(long)]
        secrets_only: bool,
        /// Force a zero exit code even if Critical/Warn findings exist —
        /// useful for human-driven exploratory runs where the gating is
        /// noise.
        #[arg(long)]
        no_fail: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PlugCommand {
    /// Verify canonical agent-plugin.toml truth against generated harness manifests.
    Sync {
        /// Check disk alignment without writing generated manifests.
        #[arg(long)]
        check: bool,
        /// Reserved for the mutating writer once the verifier is trusted.
        #[arg(long)]
        write: bool,
    },
    /// Dispatch Babel plugin reload signals after plugin definitions are rebuilt.
    Reload {
        /// Harness surface to signal.
        #[arg(long, value_enum, default_value_t = PlugReloadHost::All)]
        host: PlugReloadHost,
        /// Trace reason forwarded to Babel.
        #[arg(long, default_value = "orgmap-plugin-publish")]
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlugReloadHost {
    All,
    Claude,
    Codex,
}

impl From<PlugReloadHost> for orgmap::plugin::PluginReloadHost {
    fn from(host: PlugReloadHost) -> Self {
        match host {
            PlugReloadHost::All => Self::All,
            PlugReloadHost::Claude => Self::Claude,
            PlugReloadHost::Codex => Self::Codex,
        }
    }
}

#[derive(Debug, Serialize)]
struct MarkerResult {
    marker: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
struct NavRow {
    kind: String,
    name: String,
    path: PathBuf,
    detail: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        None => {
            let institution = orgmap::institution::load_current_institution_with_options(
                cli.config.as_deref(),
                &std::env::current_dir()?,
                orgmap::institution::LoadOptions::MAP_ONLY,
            )?;
            print_org_home(&institution);
        }
        Some(Command::List { json }) => {
            let orgs = orgmap::institution::configured_orgs(&std::env::current_dir()?);
            if json {
                print_json(&orgs)?;
            } else {
                print_org_list(&orgs);
            }
        }
        Some(Command::Work { json }) => {
            let institution = orgmap::institution::load_current_institution(
                cli.config.as_deref(),
                &std::env::current_dir()?,
            )?;
            let report = orgmap::institution::work_report(&institution);
            if json {
                print_json(&report)?;
            } else {
                print_work_report(&report);
            }
        }
        Some(Command::Intro { json }) => {
            let institution = orgmap::institution::load_current_institution_with_options(
                cli.config.as_deref(),
                &std::env::current_dir()?,
                orgmap::institution::LoadOptions::MAP_ONLY,
            )?;
            let report = orgmap::institution::intro_report(&institution);
            if json {
                print_json(&report)?;
            } else {
                print_intro_report(&report);
            }
        }
        Some(Command::Report { project, json }) => {
            let institution = filtered_institution(
                cli.config.as_deref(),
                project.as_deref(),
                orgmap::institution::LoadOptions::MAP_ONLY,
            )?;
            if json {
                print_json(&institution)?;
            } else {
                print_project_report(&institution);
            }
        }
        Some(Command::Git { project, json }) => {
            let institution = filtered_institution(
                cli.config.as_deref(),
                project.as_deref(),
                orgmap::institution::LoadOptions::WITH_GIT,
            )?;
            if json {
                print_json(&institution.projects)?;
            } else {
                print_git_report(&institution);
            }
        }
        Some(Command::Plug {
            upstream,
            json,
            command,
        }) => {
            let institution = orgmap::institution::load_current_institution(
                cli.config.as_deref(),
                &std::env::current_dir()?,
            )?;
            match command {
                Some(PlugCommand::Sync { check: _, write }) => {
                    if write {
                        return Err(
                            "org plug sync --write is not implemented yet; use --check first"
                                .into(),
                        );
                    }
                    let report = orgmap::plugin::plugin_sync_report(&institution);
                    if json {
                        print_json(&report)?;
                    } else {
                        print_plugin_sync_report(&report);
                    }
                }
                Some(PlugCommand::Reload { host, reason }) => {
                    let report = orgmap::plugin::dispatch_babel_plugin_reload(host.into(), &reason);
                    if json {
                        print_json(&report)?;
                    } else {
                        print_plugin_reload_report(&report);
                    }
                    if report.signals.iter().any(|signal| !signal.ok) {
                        std::process::exit(1);
                    }
                }
                None => {
                    let report = orgmap::plugin::plugin_report(&institution, upstream);
                    if json {
                        print_json(&report)?;
                    } else {
                        print_plugin_report(&report);
                    }
                }
            }
        }
        Some(Command::Fzf { list, cd, json }) => {
            let institution = orgmap::institution::load_current_institution_with_options(
                cli.config.as_deref(),
                &std::env::current_dir()?,
                orgmap::institution::LoadOptions::MAP_ONLY,
            )?;
            run_fzf(&institution, list, cd, json)?;
        }
        Some(Command::Identity { path }) => print_json(&orgmap::identity_for_path(&path))?,
        Some(Command::Definition { path }) => print_json(&orgmap::definition_for_path(&path))?,
        Some(Command::Stack { path }) => print_json(&orgmap::discover_workgroup_stack(&path))?,
        Some(Command::Marker { path }) => print_json(&MarkerResult {
            marker: orgmap::toml_path_for_path(&path),
        })?,
        Some(Command::Screen {
            project,
            json,
            no_gitleaks,
            secrets_only,
            no_fail,
        }) => {
            let institution = filtered_institution(
                cli.config.as_deref(),
                project.as_deref(),
                orgmap::institution::LoadOptions::MAP_ONLY,
            )?;
            let opts = orgmap::screen::ScreenOptions {
                no_gitleaks,
                secrets_only,
            };
            let report = orgmap::screen::run(&institution, &opts);
            if json {
                print_json(&report)?;
            } else {
                print_screen_report(&report);
            }
            if !no_fail && report.exit_code() != 0 {
                std::process::exit(report.exit_code());
            }
        }
    }
    Ok(())
}

fn print_plugin_reload_report(report: &orgmap::plugin::PluginReloadReport) {
    println!("org plug reload — Babel plugin reload signal");
    println!("  reason: {}", dim(&report.reason));
    for signal in &report.signals {
        let status = if signal.ok {
            styled_color(48, "ok")
        } else {
            styled_color(196, "failed")
        };
        println!(
            "  {}  {:<6}  {}",
            status.styled,
            plugin_host_label(Some(signal.host)).styled,
            signal.message
        );
        if let Some(stderr) = &signal.stderr {
            println!("      {}", dim(stderr));
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> serde_json::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn filtered_institution(
    config: Option<&std::path::Path>,
    project: Option<&str>,
    options: orgmap::institution::LoadOptions,
) -> Result<orgmap::institution::Institution, Box<dyn std::error::Error>> {
    let mut institution = orgmap::institution::load_current_institution_with_options(
        config,
        &std::env::current_dir()?,
        options,
    )?;
    if let Some(project) = project {
        institution.projects = institution
            .projects
            .into_iter()
            .filter(|row| row.name == project || row.display_name == project)
            .collect();
        if institution.projects.is_empty() {
            return Err(format!("project not found in orgmap: {project}").into());
        }
    }
    for workspace in &mut institution.workspaces {
        workspace.projects = institution
            .projects
            .iter()
            .filter(|project| project.section == workspace.key && !project.blacklisted)
            .count();
    }
    Ok(institution)
}

fn run_fzf(
    institution: &orgmap::institution::Institution,
    list: bool,
    cd: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let rows = nav_rows(institution);
    if json {
        if list {
            print_json(&rows)?;
            return Ok(());
        }
    }
    if list {
        for row in &rows {
            println!("{}", nav_row_line(row));
        }
        return Ok(());
    }

    let selected = select_nav_row(&rows)?;
    if json {
        print_json(&selected)?;
    } else if cd {
        println!("cd {}", shell_quote(&selected.path));
    } else {
        println!("{}", selected.path.display());
    }
    Ok(())
}

fn nav_rows(institution: &orgmap::institution::Institution) -> Vec<NavRow> {
    let mut rows = Vec::new();
    rows.push(NavRow {
        kind: "root".to_string(),
        name: institution.gh_org.clone(),
        path: institution.root.clone(),
        detail: "org root".to_string(),
    });
    rows.push(NavRow {
        kind: "config".to_string(),
        name: "orgmap.toml".to_string(),
        path: institution
            .config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| institution.root.clone()),
        detail: display_path(&institution.config_path),
    });
    for workspace in institution
        .workspaces
        .iter()
        .filter(|workspace| !workspace.internal)
    {
        let Some(path) = &workspace.root else {
            continue;
        };
        rows.push(NavRow {
            kind: "workgroup".to_string(),
            name: workspace.key.clone(),
            path: path.clone(),
            detail: workspace.display_name.clone(),
        });
    }
    for project in institution
        .projects
        .iter()
        .filter(|project| project.local && !project.blacklisted)
    {
        let Some(path) = &project.path else {
            continue;
        };
        rows.push(NavRow {
            kind: "project".to_string(),
            name: project.name.clone(),
            path: path.clone(),
            detail: format!(
                "{} {}",
                project.section,
                project.description.as_deref().unwrap_or("")
            )
            .trim()
            .to_string(),
        });
    }
    rows.sort_by(|a, b| {
        nav_kind_rank(&a.kind)
            .cmp(&nav_kind_rank(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

fn select_nav_row(rows: &[NavRow]) -> Result<NavRow, Box<dyn std::error::Error>> {
    let mut child = ProcessCommand::new("fzf")
        .args([
            "--ansi",
            "--delimiter",
            "\t",
            "--with-nth",
            "1,2,4,3",
            "--prompt",
            "org> ",
            "--height",
            "80%",
            "--reverse",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to launch fzf: {error}"))?;

    {
        let Some(stdin) = child.stdin.as_mut() else {
            return Err("failed to open fzf stdin".into());
        };
        for row in rows {
            writeln!(stdin, "{}", nav_row_line(row))?;
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("fzf selection cancelled".into());
    }
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = selected
        .split('\t')
        .nth(2)
        .ok_or("fzf returned an invalid row")?;
    rows.iter()
        .find(|row| row.path == PathBuf::from(path))
        .cloned()
        .ok_or_else(|| "selected path no longer exists in nav rows".into())
}

fn nav_row_line(row: &NavRow) -> String {
    format!(
        "{:<9}\t{:<24}\t{}\t{}",
        row.kind,
        row.name,
        row.path.display(),
        row.detail
    )
}

fn nav_kind_rank(kind: &str) -> u8 {
    match kind {
        "root" => 0,
        "config" => 1,
        "workgroup" => 2,
        "project" => 3,
        _ => 4,
    }
}

fn shell_quote(path: &Path) -> String {
    let text = path.display().to_string();
    if text.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn print_org_home(institution: &orgmap::institution::Institution) {
    println!(
        "{} {}",
        color_bold(39, "org"),
        color(39, &format!("— {}", institution.gh_org))
    );
    println!("  root: {}", display_path(&institution.root));
    println!("  config: {}", display_path(&institution.config_path));
    println!(
        "  graph: {} workgroups · {} projects · {} local",
        institution
            .workspaces
            .iter()
            .filter(|workspace| !workspace.internal)
            .count(),
        institution
            .projects
            .iter()
            .filter(|project| !project.blacklisted)
            .count(),
        institution
            .projects
            .iter()
            .filter(|project| project.local && !project.blacklisted)
            .count()
    );
    println!();
    println!("{}", dim("workgroups"));
    for workspace in institution
        .workspaces
        .iter()
        .filter(|workspace| !workspace.internal)
    {
        let ansi = workspace_ansi(workspace);
        let title = if workspace.emoji.is_empty() {
            workspace.display_name.clone()
        } else {
            format!("{} {}", workspace.emoji, workspace.display_name)
        };
        let root = workspace
            .root
            .as_ref()
            .map(|path| display_path(path))
            .unwrap_or_else(|| "-".to_string());
        let stage_counts = workspace_stage_counts(institution, &workspace.key);
        println!(
            "  {} {:<16} {:<28} {:>3} projects  {}  {}",
            color(ansi, "■"),
            color_bold(ansi, &workspace.key),
            title,
            workspace.projects,
            dim(&stage_counts),
            dim(&root)
        );
    }
    println!();
    print_command_surface();
}

fn print_command_surface() {
    println!("{}", dim("fluent surface"));
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(45, "report [project]"),
        dim("->"),
        dim("public/readme shape")
    );
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(214, "git [project]"),
        dim("->"),
        dim("dirty/ahead/behind local repos")
    );
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(141, "plug"),
        dim("->"),
        dim("agent plugin carriers")
    );
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(75, "fzf"),
        dim("->"),
        dim("cd navigation picker")
    );
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(196, "screen"),
        dim("->"),
        dim("anti-sloppy normalizer (secrets, paths, dbg!) — pre-publish gate")
    );
    println!(
        "  {} {} {} {} {}",
        color_bold(39, "org"),
        dim("->"),
        color_bold(81, "intro | work | list | stack"),
        dim("->"),
        dim("familiarization, active work, registry, markers")
    );
}

fn print_org_list(orgs: &[orgmap::institution::OrgListing]) {
    println!("org — configured institutions");
    if let Some(path) = orgmap::institution::config_dir_path() {
        println!("  config dir: {}", display_path(&path));
    }
    println!("  env: ORGMAP_NAME, ORGMAP_ROOT, ORGMAP_CONFIG");
    println!();

    if orgs.is_empty() {
        println!("No organizations configured.");
        println!("Create ~/.config/orgmap/config/<name>.toml with root/config fields.");
        println!("Agents may use ORGMAP_NAME + ORGMAP_ROOT or ORGMAP_CONFIG.");
        return;
    }

    for org in orgs {
        let default = if org.default { "*" } else { " " };
        let active = if org.active { "@" } else { " " };
        let root = org
            .root
            .as_ref()
            .map(|path| display_path(path))
            .unwrap_or_else(|| "-".to_string());
        let config = org
            .config_path
            .as_ref()
            .map(|path| display_path(path))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}{} {:<18} {:<28} {:<28} {}",
            default, active, org.name, root, config, org.source
        );
    }
}

fn print_project_report(institution: &orgmap::institution::Institution) {
    println!("org report — {}", institution.gh_org);
    println!("  root: {}", display_path(&institution.root));
    println!();

    for workspace in institution
        .workspaces
        .iter()
        .filter(|workspace| !workspace.internal && workspace.projects > 0)
    {
        let ansi = workspace_ansi(workspace);
        let title = if workspace.emoji.is_empty() {
            workspace.display_name.clone()
        } else {
            format!("{} {}", workspace.emoji, workspace.display_name)
        };
        println!(
            "{} {} {}",
            color(ansi, "──"),
            color_bold(ansi, &workspace.key),
            color(ansi, &title)
        );

        let mut projects = institution
            .projects
            .iter()
            .filter(|project| project.section == workspace.key && !project.blacklisted)
            .collect::<Vec<_>>();
        projects.sort_by(|a, b| a.name.cmp(&b.name));
        for project in projects {
            let local = if project.local {
                color(141, "local")
            } else {
                dim("upstream")
            };
            let git = project
                .git
                .as_ref()
                .map(git_badge)
                .unwrap_or_else(|| dim("no-git"));
            let stage = stage_badge(project.stage.as_deref());
            let path = project
                .path
                .as_ref()
                .map(|path| display_path(path))
                .unwrap_or_default();
            let description = project.description.as_deref().unwrap_or("");
            println!(
                "  {:<18} {:<12} {:<12} {:<14} {} {}",
                project.name,
                stage,
                local,
                git,
                dim(&path),
                description
            );
        }
        println!();
    }
}

fn print_git_report(institution: &orgmap::institution::Institution) {
    let mut projects = institution
        .projects
        .iter()
        .filter(|project| project.local && !project.blacklisted)
        .collect::<Vec<_>>();
    projects.sort_by(|a, b| {
        git_state_rank(a)
            .cmp(&git_state_rank(b))
            .then_with(|| a.name.cmp(&b.name))
    });

    println!("org git — {}", institution.gh_org);
    println!(
        "  {} local repos · {} dirty · {} ahead · {} behind · {} no upstream",
        projects.len(),
        projects
            .iter()
            .filter(|project| project.git.as_ref().is_some_and(|git| git.dirty > 0))
            .count(),
        projects
            .iter()
            .filter(|project| project
                .git
                .as_ref()
                .is_some_and(|git| git.ahead.unwrap_or(0) > 0))
            .count(),
        projects
            .iter()
            .filter(|project| project
                .git
                .as_ref()
                .is_some_and(|git| git.behind.unwrap_or(0) > 0))
            .count(),
        projects
            .iter()
            .filter(|project| project.git.as_ref().is_some_and(|git| !git.has_upstream))
            .count()
    );
    println!();

    let mut current = String::new();
    for project in projects {
        let label = git_state_label(project);
        if label != current {
            current = label.clone();
            println!("{} {}", dim("──"), git_state_title(&label));
        }
        let Some(git) = project.git.as_ref() else {
            continue;
        };
        let path = project
            .path
            .as_ref()
            .map(|path| display_path(path))
            .unwrap_or_default();
        println!(
            "  {:<18} {:<16} {:<4} {:<4} {:<4} {:<8} {}",
            project.name,
            color(45, &git.branch),
            count_cell("↑", git.ahead.unwrap_or(0), 214),
            count_cell("↓", git.behind.unwrap_or(0), 39),
            count_cell("✦", git.dirty, 196),
            dim(&abbreviate_age(git.age.as_deref().unwrap_or(""))),
            dim(&path)
        );
    }
}

fn print_plugin_report(report: &orgmap::plugin::PluginReport) {
    println!("org plug — agent plugin carriers");
    println!(
        "  {} projects · {} surfaces · {} Claude · {} Codex · {} invalid",
        report.projects, report.surfaces, report.claude, report.codex, report.invalid
    );
    if let Some(upstream) = &report.upstream {
        println!(
            "  upstream: {} synced · {} changed · {} missing · {} errors",
            color(48, &upstream.synced.to_string()),
            color(214, &upstream.changed.to_string()),
            color(196, &upstream.missing.to_string()),
            color(196, &upstream.error.to_string())
        );
        println!(
            "  carriers: {} dirty · {} ahead · {} behind · {} no upstream",
            color(196, &upstream.dirty.to_string()),
            color(214, &upstream.ahead.to_string()),
            color(39, &upstream.behind.to_string()),
            dim(&upstream.no_upstream.to_string())
        );
    }
    println!();
    if report.plugins.is_empty() {
        println!("No agent plugin manifests found.");
        return;
    }

    let rows = report
        .plugins
        .iter()
        .map(plugin_report_row)
        .collect::<Vec<_>>();
    let name_w = rows.iter().map(|row| row.name.raw.len()).max().unwrap_or(4);
    let version_w = rows
        .iter()
        .map(|row| row.version.raw.len())
        .max()
        .unwrap_or(7);
    let caps_w = rows.iter().map(|row| row.caps.raw.len()).max().unwrap_or(4);
    let carrier_w = rows
        .iter()
        .map(|row| row.carrier.raw.len())
        .max()
        .unwrap_or(7);
    let upstream_w = rows
        .iter()
        .map(|row| row.upstream.raw.len())
        .max()
        .unwrap_or(8);

    let mut current = "";
    for plugin in &report.plugins {
        if plugin.project != current {
            current = &plugin.project;
            println!("{} {}", dim("──"), color_bold(39, &plugin.project));
        }
        let row = plugin_report_row(plugin);
        let manifest = display_path(&plugin.manifest);
        if plugin.valid {
            println!(
                "  {}  {}  {}  {}  {}  {}  {}",
                pad_cell(&row.host, 6),
                pad_cell(&row.name, name_w),
                pad_cell(&row.version, version_w),
                pad_cell(&row.caps, caps_w),
                pad_cell(&row.carrier, carrier_w),
                pad_cell(&row.upstream, upstream_w),
                dim(&manifest)
            );
        } else {
            println!(
                "  {}  {}  {}  {}  {}  {}  {} {}",
                pad_cell(&row.host, 6),
                pad_cell(&row.name, name_w),
                pad_cell(&row.version, version_w),
                pad_cell(&row.caps, caps_w),
                pad_cell(&row.carrier, carrier_w),
                pad_cell(&row.upstream, upstream_w),
                color(196, plugin.error.as_deref().unwrap_or("invalid manifest")),
                dim(&manifest)
            );
        }
    }
}

fn print_plugin_sync_report(report: &orgmap::plugin::PluginSyncReport) {
    println!("org plug sync --check — canonical plugin truth");
    println!(
        "  {} projects · {} truth files · {} provider manifests",
        report.projects, report.truth_files, report.provider_manifests
    );
    println!(
        "  {} aligned · {} would write · {} need truth · {} invalid truth",
        color(48, &report.aligned.to_string()),
        color(214, &report.would_write.to_string()),
        color(196, &report.missing_truth.to_string()),
        color(196, &report.invalid_truth.to_string())
    );
    println!();
    if report.plans.is_empty() {
        println!("No plugin truth or provider manifests found.");
        return;
    }

    let action_w = report
        .plans
        .iter()
        .map(|plan| plugin_sync_action_label(plan.action).raw.len())
        .max()
        .unwrap_or(6);
    let plugin_w = report
        .plans
        .iter()
        .map(|plan| plan.plugin.len())
        .max()
        .unwrap_or(6);
    let host_w = report
        .plans
        .iter()
        .map(|plan| plugin_host_label(plan.host).raw.len())
        .max()
        .unwrap_or(4);

    let mut current = "";
    for plan in &report.plans {
        if plan.project != current {
            current = &plan.project;
            println!("{} {}", dim("──"), color_bold(39, &plan.project));
        }
        let action = plugin_sync_action_label(plan.action);
        let host = plugin_host_label(plan.host);
        let manifest = plan
            .manifest
            .as_ref()
            .map(|path| display_path(path))
            .or_else(|| plan.truth.as_ref().map(|path| display_path(path)))
            .unwrap_or_default();
        println!(
            "  {}  {:<plugin_w$}  {}  {:<28}  {}",
            pad_cell(&action, action_w),
            plan.plugin,
            pad_cell(&host, host_w),
            plan.message,
            dim(&manifest)
        );
    }
}

struct StyledCell {
    raw: String,
    styled: String,
}

struct PluginReportRow {
    host: StyledCell,
    name: StyledCell,
    version: StyledCell,
    caps: StyledCell,
    carrier: StyledCell,
    upstream: StyledCell,
}

fn plugin_report_row(plugin: &orgmap::plugin::PluginSurface) -> PluginReportRow {
    let host = match plugin.host {
        orgmap::plugin::PluginHost::Claude => StyledCell {
            raw: "claude".to_string(),
            styled: color_bold(214, "claude"),
        },
        orgmap::plugin::PluginHost::Codex => StyledCell {
            raw: "codex".to_string(),
            styled: color_bold(48, "codex"),
        },
    };
    let name = plugin.name.as_deref().unwrap_or("invalid").to_string();
    let name = if plugin.valid {
        StyledCell {
            raw: name.clone(),
            styled: name,
        }
    } else {
        StyledCell {
            raw: name.clone(),
            styled: color_bold(196, &name),
        }
    };
    let version = plugin.version.as_deref().unwrap_or("-").to_string();
    PluginReportRow {
        host,
        name,
        version: StyledCell {
            raw: version.clone(),
            styled: version,
        },
        caps: plugin_caps(plugin),
        carrier: plugin_carrier_badge(plugin),
        upstream: plugin_upstream_badge(plugin),
    }
}

fn plugin_sync_action_label(action: orgmap::plugin::PluginSyncAction) -> StyledCell {
    match action {
        orgmap::plugin::PluginSyncAction::Aligned => styled_color(48, "aligned"),
        orgmap::plugin::PluginSyncAction::WouldWrite => styled_color(214, "would-write"),
        orgmap::plugin::PluginSyncAction::MissingManifest => styled_color(214, "missing"),
        orgmap::plugin::PluginSyncAction::InitTruth => styled_color(196, "init-truth"),
        orgmap::plugin::PluginSyncAction::InvalidTruth => styled_color(196, "invalid"),
    }
}

fn plugin_host_label(host: Option<orgmap::plugin::PluginHost>) -> StyledCell {
    match host {
        Some(orgmap::plugin::PluginHost::Claude) => StyledCell {
            raw: "claude".to_string(),
            styled: color_bold(214, "claude"),
        },
        Some(orgmap::plugin::PluginHost::Codex) => StyledCell {
            raw: "codex".to_string(),
            styled: color_bold(48, "codex"),
        },
        None => styled_dim("-"),
    }
}

fn pad_cell(cell: &StyledCell, width: usize) -> String {
    let pad = width.saturating_sub(cell.raw.len());
    format!("{}{}", cell.styled, " ".repeat(pad))
}

fn print_work_report(report: &orgmap::institution::WorkReport) {
    println!("org work — {}", report.gh_org);
    println!("  root: {}", display_path(&report.root));
    println!("  config: {}", display_path(&report.config_path));
    println!(
        "  graph: {} workspaces · {} projects · {} local",
        report.workspaces, report.projects, report.local_projects
    );
    println!();
    print_project_group("dirty", &report.dirty, |git| git.dirty.to_string());
    print_project_group("ahead", &report.ahead, |git| {
        format!("↑{}", git.ahead.unwrap_or(0))
    });
    print_project_group("no upstream", &report.no_upstream, |_| {
        "no-upstream".to_string()
    });
}

fn print_project_group<F>(label: &str, projects: &[orgmap::institution::Project], detail: F)
where
    F: Fn(&orgmap::institution::GitState) -> String,
{
    println!("{label}: {}", projects.len());
    for project in projects.iter().take(24) {
        let git = project.git.as_ref();
        let detail = git.map(&detail).unwrap_or_default();
        println!(
            "  {:<24} {:<12} {:<12} {}",
            project.name,
            project.section,
            detail,
            project
                .path
                .as_ref()
                .map(|path| display_path(path))
                .unwrap_or_default()
        );
    }
    if projects.len() > 24 {
        println!("  ... {} more", projects.len() - 24);
    }
    println!();
}

fn print_screen_report(report: &orgmap::screen::ScreenReport) {
    println!("{} — {}", color_bold(196, "org screen"), report.gh_org);
    println!("  root: {}", display_path(&report.root));
    println!(
        "  {} scanned · {} skipped · gitleaks: {}",
        report.projects_scanned,
        report.projects_skipped,
        if report.gitleaks_available {
            color(48, "on")
        } else {
            dim("off")
        }
    );
    println!(
        "  totals: {} · {} · {}",
        if report.critical > 0 {
            color_bold(196, &format!("❌ {} secret", report.critical))
        } else {
            dim("❌ 0 secret")
        },
        if report.warn > 0 {
            color_bold(214, &format!("⚠️  {} pathleak", report.warn))
        } else {
            dim("⚠️  0 pathleak")
        },
        if report.info > 0 {
            color(39, &format!("ℹ️  {} sloppy", report.info))
        } else {
            dim("ℹ️  0 sloppy")
        }
    );
    println!();

    let dirty: Vec<&orgmap::screen::ProjectFindings> = report
        .projects
        .iter()
        .filter(|p| p.critical + p.warn + p.info > 0 || p.gitleaks_error.is_some())
        .collect();

    if dirty.is_empty() {
        println!("  {}", color_bold(48, "clean — every project passes screen ✔"));
        return;
    }

    for pf in &dirty {
        let badge = format!(
            "{} {} · {} · {}",
            color_bold(39, &pf.name),
            count_badge("❌", pf.critical, 196),
            count_badge("⚠️", pf.warn, 214),
            count_badge("ℹ️", pf.info, 39)
        );
        println!("{} {}", dim("──"), badge);
        if let Some(err) = &pf.gitleaks_error {
            println!("    {} gitleaks: {}", color(196, "!"), err);
        }
        let grouped = orgmap::screen::group_by_file(pf);
        for (file, findings) in &grouped {
            println!("    {}", color_bold(45, file));
            for finding in findings {
                let sev = match finding.severity {
                    orgmap::screen::Severity::Critical => color_bold(196, "❌ secret  "),
                    orgmap::screen::Severity::Warn => color_bold(214, "⚠️  pathleak"),
                    orgmap::screen::Severity::Info => color(39, "ℹ️  sloppy  "),
                };
                let src = match finding.source {
                    orgmap::screen::FindingSource::Pattern => dim(&finding.pattern_id),
                    orgmap::screen::FindingSource::Gitleaks => color(141, &finding.pattern_id),
                };
                println!(
                    "      {}  {:>4}  {}  {}",
                    sev,
                    color(81, &format!("L{}", finding.line)),
                    src,
                    dim(&finding.snippet)
                );
            }
        }
        if pf.truncated > 0 {
            println!(
                "    {}",
                dim(&format!(
                    "… {} more non-critical findings suppressed (cap {}). Run with `--json` for the full list.",
                    pf.truncated,
                    orgmap::screen::MAX_FINDINGS_PER_PROJECT_PUBLIC
                ))
            );
        }
        println!();
    }
}

fn count_badge(glyph: &str, value: usize, ansi: u8) -> String {
    if value > 0 {
        color(ansi, &format!("{glyph} {value}"))
    } else {
        dim(&format!("{glyph} 0"))
    }
}

fn print_intro_report(report: &orgmap::institution::IntroReport) {
    println!("org intro — {}", report.gh_org);
    println!("  root: {}", display_path(&report.root));
    println!();

    for workspace in &report.workspaces {
        if workspace.projects.is_empty() {
            continue;
        }
        let title = if workspace.emoji.is_empty() {
            workspace.display_name.clone()
        } else {
            format!("{} {}", workspace.emoji, workspace.display_name)
        };
        println!("{} ({})", title, workspace.key);
        if let Some(subtitle) = &workspace.subtitle {
            println!("  {}", subtitle);
        }
        for project in workspace.projects.iter().take(18) {
            let stage = project.stage.as_deref().unwrap_or("");
            let description = project.description.as_deref().unwrap_or("");
            println!(
                "  {:<18} {:<11} {}",
                project.display_name, stage, description
            );
        }
        if workspace.projects.len() > 18 {
            println!("  ... {} more", workspace.projects.len() - 18);
        }
        println!();
    }
}

fn workspace_stage_counts(
    institution: &orgmap::institution::Institution,
    workspace: &str,
) -> String {
    let projects = institution
        .projects
        .iter()
        .filter(|project| project.section == workspace && !project.blacklisted);
    let mut certified = 0;
    let mut beta = 0;
    let mut research = 0;
    let mut hazard = 0;
    for project in projects {
        match project.stage.as_deref() {
            Some("certified") => certified += 1,
            Some("beta") => beta += 1,
            Some("research") => research += 1,
            Some("hazard-low" | "hazard-high") => hazard += 1,
            _ => {}
        }
    }
    format!("cert {certified} beta {beta} research {research} hazard {hazard}")
}

fn workspace_ansi(workspace: &orgmap::institution::Workspace) -> u8 {
    workspace
        .root
        .as_ref()
        .and_then(|root| orgmap::definition_for_path(root).and_then(|definition| definition.ansi256))
        .unwrap_or_else(|| orgmap::auto_workgroup_ansi256(&workspace.key))
}

fn stage_badge(stage: Option<&str>) -> String {
    match stage {
        Some("certified") => color_bold(39, "certified"),
        Some("beta") => color(48, "beta"),
        Some("research") => color(214, "research"),
        Some("hazard-low") => color(208, "hazard-low"),
        Some("hazard-high") => color_bold(196, "hazard-high"),
        Some("archived") => dim("archived"),
        Some(other) => color(141, other),
        None => dim("unstaged"),
    }
}

fn git_badge(git: &orgmap::institution::GitState) -> String {
    if git.dirty > 0 {
        return color(196, &format!("dirty {}", git.dirty));
    }
    if git.ahead.unwrap_or(0) > 0 {
        return color(214, &format!("ahead {}", git.ahead.unwrap_or(0)));
    }
    if git.behind.unwrap_or(0) > 0 {
        return color(39, &format!("behind {}", git.behind.unwrap_or(0)));
    }
    if !git.has_upstream {
        return dim("no-upstream");
    }
    color(48, "clean")
}

fn git_state_rank(project: &orgmap::institution::Project) -> u8 {
    match git_state_label(project).as_str() {
        "dirty+diverged" => 0,
        "diverged" => 1,
        "dirty" => 2,
        "ahead" => 3,
        "behind" => 4,
        "no-upstream" => 5,
        "clean" => 6,
        _ => 7,
    }
}

fn git_state_label(project: &orgmap::institution::Project) -> String {
    let Some(git) = project.git.as_ref() else {
        return "no-git".to_string();
    };
    let ahead = git.ahead.unwrap_or(0);
    let behind = git.behind.unwrap_or(0);
    if !git.has_upstream {
        "no-upstream".to_string()
    } else if git.dirty > 0 && (ahead > 0 || behind > 0) {
        "dirty+diverged".to_string()
    } else if git.dirty > 0 {
        "dirty".to_string()
    } else if ahead > 0 && behind > 0 {
        "diverged".to_string()
    } else if ahead > 0 {
        "ahead".to_string()
    } else if behind > 0 {
        "behind".to_string()
    } else {
        "clean".to_string()
    }
}

fn git_state_title(label: &str) -> String {
    match label {
        "dirty+diverged" => color_bold(196, "DIRTY + DIVERGED"),
        "diverged" => color_bold(201, "DIVERGED"),
        "dirty" => color_bold(196, "DIRTY"),
        "ahead" => color_bold(214, "AHEAD"),
        "behind" => color_bold(39, "BEHIND"),
        "no-upstream" => dim("NO UPSTREAM"),
        "clean" => color_bold(48, "CLEAN"),
        _ => dim(label),
    }
}

fn count_cell(glyph: &str, value: usize, ansi: u8) -> String {
    if value > 0 {
        color(ansi, &format!("{glyph}{value}"))
    } else {
        dim(&format!("{glyph}·"))
    }
}

fn abbreviate_age(age: &str) -> String {
    let mut parts = age.split_whitespace();
    let Some(amount) = parts.next() else {
        return String::new();
    };
    let Some(unit) = parts.next() else {
        return age.to_string();
    };
    let unit = match unit {
        "second" | "seconds" => "s",
        "minute" | "minutes" => "m",
        "hour" | "hours" => "h",
        "day" | "days" => "d",
        "week" | "weeks" => "w",
        "month" | "months" => "mo",
        "year" | "years" => "y",
        _ => return age.to_string(),
    };
    format!("{amount}{unit}")
}

fn plugin_caps(plugin: &orgmap::plugin::PluginSurface) -> StyledCell {
    let mut caps = Vec::new();
    if let Some(feature) = &plugin.skills {
        caps.push(feature_chip("skills", feature.count, feature.exists));
    }
    if let Some(feature) = &plugin.mcp_servers {
        caps.push(feature_chip("mcp", feature.count, feature.exists));
    }
    if let Some(feature) = &plugin.lsp_servers {
        caps.push(feature_chip("lsp", feature.count, feature.exists));
    }
    if let Some(feature) = &plugin.hooks {
        caps.push(feature_chip("hooks", feature.count, feature.exists));
    }
    if plugin.interface {
        caps.push(StyledCell {
            raw: "ui".to_string(),
            styled: color(81, "ui"),
        });
    }
    if caps.is_empty() {
        StyledCell {
            raw: "metadata-only".to_string(),
            styled: dim("metadata-only"),
        }
    } else {
        join_cells(&caps, " ")
    }
}

fn plugin_carrier_badge(plugin: &orgmap::plugin::PluginSurface) -> StyledCell {
    let Some(git) = plugin.carrier_git.as_ref() else {
        return styled_dim("no-git");
    };
    let ahead = git.ahead.unwrap_or(0);
    let behind = git.behind.unwrap_or(0);
    if git.dirty > 0 {
        if ahead > 0 {
            return styled_color(196, &format!("dirty:{}↑{}", git.dirty, ahead));
        }
        if behind > 0 {
            return styled_color(196, &format!("dirty:{}↓{}", git.dirty, behind));
        }
        return styled_color(196, &format!("dirty:{}", git.dirty));
    }
    if ahead > 0 {
        return styled_color(214, &format!("ahead:{ahead}"));
    }
    if behind > 0 {
        return styled_color(39, &format!("behind:{behind}"));
    }
    if !git.has_upstream {
        return styled_dim("no-upstream");
    }
    styled_color(48, "repo-synced")
}

fn plugin_upstream_badge(plugin: &orgmap::plugin::PluginSurface) -> StyledCell {
    let Some(upstream) = plugin.upstream.as_ref() else {
        return styled_dim("local-only");
    };
    match upstream.status {
        orgmap::plugin::PluginUpstreamStatus::Synced => styled_color(48, "manifest-ok"),
        orgmap::plugin::PluginUpstreamStatus::Changed => styled_color(214, "manifest-diff"),
        orgmap::plugin::PluginUpstreamStatus::Missing => styled_color(196, "manifest-missing"),
        orgmap::plugin::PluginUpstreamStatus::Error => styled_color(196, "gh-error"),
    }
}

fn feature_chip(label: &str, count: usize, exists: bool) -> StyledCell {
    let text = if count > 0 {
        format!("{label}:{count}")
    } else {
        label.to_string()
    };
    if exists {
        styled_color(48, &text)
    } else {
        styled_color(196, &text)
    }
}

fn join_cells(cells: &[StyledCell], sep: &str) -> StyledCell {
    StyledCell {
        raw: cells
            .iter()
            .map(|cell| cell.raw.as_str())
            .collect::<Vec<_>>()
            .join(sep),
        styled: cells
            .iter()
            .map(|cell| cell.styled.as_str())
            .collect::<Vec<_>>()
            .join(sep),
    }
}

fn styled_color(ansi: u8, text: &str) -> StyledCell {
    StyledCell {
        raw: text.to_string(),
        styled: color(ansi, text),
    }
}

fn styled_dim(text: &str) -> StyledCell {
    StyledCell {
        raw: text.to_string(),
        styled: dim(text),
    }
}

fn color(ansi: u8, text: &str) -> String {
    format!("\x1b[38;5;{ansi}m{text}\x1b[0m")
}

fn color_bold(ansi: u8, text: &str) -> String {
    format!("\x1b[1;38;5;{ansi}m{text}\x1b[0m")
}

fn dim(text: &str) -> String {
    format!("\x1b[2m{text}\x1b[0m")
}

fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home).display().to_string();
        if let Some(rest) = text.strip_prefix(&home) {
            return format!("~{}", rest);
        }
    }
    text
}
