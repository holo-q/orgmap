use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "orgmap")]
#[command(about = "Query org/workgroup markers as JSON")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve the nearest workgroup identity for a path.
    Identity {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Resolve the nearest full workgroup definition for a path.
    Definition {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Resolve every ancestor workgroup marker for a path.
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
