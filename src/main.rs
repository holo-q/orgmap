use std::path::PathBuf;

use clap::{Parser, Subcommand};
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
        /// Emit JSON instead of terminal text.
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
}

#[derive(Debug, Serialize)]
struct MarkerResult {
    marker: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        None => {
            let institution = orgmap::institution::load_current_institution(
                cli.config.as_deref(),
                &std::env::current_dir()?,
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
            let institution = orgmap::institution::load_current_institution(
                cli.config.as_deref(),
                &std::env::current_dir()?,
            )?;
            let report = orgmap::institution::intro_report(&institution);
            if json {
                print_json(&report)?;
            } else {
                print_intro_report(&report);
            }
        }
        Some(Command::Report { project, json }) => {
            let institution = filtered_institution(cli.config.as_deref(), project.as_deref())?;
            if json {
                print_json(&institution)?;
            } else {
                print_project_report(&institution);
            }
        }
        Some(Command::Git { project, json }) => {
            let institution = filtered_institution(cli.config.as_deref(), project.as_deref())?;
            if json {
                print_json(&institution.projects)?;
            } else {
                print_git_report(&institution);
            }
        }
        Some(Command::Plug { json }) => {
            let institution = orgmap::institution::load_current_institution(
                cli.config.as_deref(),
                &std::env::current_dir()?,
            )?;
            let report = orgmap::plugin::plugin_report(&institution);
            if json {
                print_json(&report)?;
            } else {
                print_plugin_report(&report);
            }
        }
        Some(Command::Identity { path }) => print_json(&orgmap::identity_for_path(&path))?,
        Some(Command::Definition { path }) => print_json(&orgmap::definition_for_path(&path))?,
        Some(Command::Stack { path }) => print_json(&orgmap::discover_workgroup_stack(&path))?,
        Some(Command::Marker { path }) => print_json(&MarkerResult {
            marker: orgmap::toml_path_for_path(&path),
        })?,
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> serde_json::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn filtered_institution(
    config: Option<&std::path::Path>,
    project: Option<&str>,
) -> Result<orgmap::institution::Institution, Box<dyn std::error::Error>> {
    let mut institution =
        orgmap::institution::load_current_institution(config, &std::env::current_dir()?)?;
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
    println!();
    if report.plugins.is_empty() {
        println!("No agent plugin manifests found.");
        return;
    }

    let mut current = "";
    for plugin in &report.plugins {
        if plugin.project != current {
            current = &plugin.project;
            println!("{} {}", dim("──"), color_bold(39, &plugin.project));
        }
        let host = match plugin.host {
            orgmap::plugin::PluginHost::Claude => color_bold(214, "claude"),
            orgmap::plugin::PluginHost::Codex => color_bold(48, "codex"),
        };
        let name = plugin.name.as_deref().unwrap_or("invalid");
        let version = plugin.version.as_deref().unwrap_or("-");
        let caps = plugin_caps(plugin);
        let manifest = display_path(&plugin.manifest);
        if plugin.valid {
            println!(
                "  {:<8} {:<20} {:<8} {:<34} {}",
                host,
                name,
                version,
                caps,
                dim(&manifest)
            );
        } else {
            println!(
                "  {:<8} {:<20} {:<8} {} {}",
                host,
                color_bold(196, name),
                version,
                color(196, plugin.error.as_deref().unwrap_or("invalid manifest")),
                dim(&manifest)
            );
        }
    }
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

fn plugin_caps(plugin: &orgmap::plugin::PluginSurface) -> String {
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
        caps.push(color(81, "ui"));
    }
    if caps.is_empty() {
        dim("metadata-only")
    } else {
        caps.join(" ")
    }
}

fn feature_chip(label: &str, count: usize, exists: bool) -> String {
    let text = if count > 0 {
        format!("{label}:{count}")
    } else {
        label.to_string()
    };
    if exists {
        color(48, &text)
    } else {
        color(196, &text)
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
