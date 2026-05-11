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
    match cli.command.unwrap_or(Command::Work { json: false }) {
        Command::Work { json } => {
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
        Command::Intro { json } => {
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
        Command::Identity { path } => print_json(&orgmap::identity_for_path(&path))?,
        Command::Definition { path } => print_json(&orgmap::definition_for_path(&path))?,
        Command::Stack { path } => print_json(&orgmap::discover_workgroup_stack(&path))?,
        Command::Marker { path } => print_json(&MarkerResult {
            marker: orgmap::toml_path_for_path(&path),
        })?,
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> serde_json::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
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
