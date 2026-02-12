use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "sweeper", version, about = "Organize files and clean stale projects safely")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Organize files in a folder by type
    Organize {
        path: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },

    /// Scan for stale project folders
    Scan {
        path: PathBuf,
        #[arg(long, default_value_t = 30)]
        older_than: u64,
    },

    /// Archive stale project folders into YYYY-MM buckets
    Archive {
        path: PathBuf,
        #[arg(long)]
        dest: PathBuf,
        #[arg(long, default_value_t = 30)]
        older_than: u64,
        #[arg(long)]
        yes: bool,
    },

    /// Send stale project folders to system bin (safe delete)
    Delete {
        path: PathBuf,
        #[arg(long, default_value_t = 90)]
        older_than: u64,
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Organize { path, dry_run } => {
            sweeper::organize_folder(&path, dry_run)?;
        }
        Commands::Scan { path, older_than } => {
            let report = sweeper::scan_projects(&path, older_than)?;
            sweeper::print_report(&report);
        }
        Commands::Archive {
            path,
            dest,
            older_than,
            yes,
        } => {
            let report = sweeper::scan_projects(&path, older_than)?;
            let plan = sweeper::build_archive_plan(&report, &dest)?;
            sweeper::print_plan(&plan);

            if yes {
                sweeper::apply_archive_plan(&plan)?;
                println!("\nArchived successfully.");
            } else {
                println!("\nDry-run only. Use --yes to apply.");
            }
        }
        Commands::Delete {
            path,
            older_than,
            yes,
        } => {
            let report = sweeper::scan_projects(&path, older_than)?;

            if report.stale.is_empty() {
                println!("Nothing to delete.");
                return Ok(());
            }

            sweeper::print_report(&report);

            if yes {
                sweeper::delete_to_trash(&report.stale)?;
                println!("\nMoved to system bin successfully.");
            } else {
                println!("\nDry-run only. Use --yes to move to bin.");
            }
        }
    }

    Ok(())
}
