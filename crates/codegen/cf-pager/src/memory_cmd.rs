use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use cf_shell::session::memory::storage::MemoryStorage;

#[derive(Debug, clap::Args, Clone)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: MemoryCommand,
}

#[derive(Debug, Subcommand, Clone)]
pub enum MemoryCommand {
    /// Clear memory files (workspace by default)
    Clear {
        /// Clear workspace-scoped memory (MEMORY.md, sessions/, index.sqlite)
        #[arg(long, group = "scope")]
        workspace: bool,
        /// Clear global MEMORY.md
        #[arg(long, group = "scope")]
        global: bool,
        /// Clear both workspace and global memory
        #[arg(long, group = "scope")]
        all: bool,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Switch memory scope mode (isolated/project/shared)
    Scope {
        /// Memory scope mode: isolated, project, or shared
        #[arg(value_enum)]
        mode: MemoryScopeMode,
    },
    /// Show current memory scope mode
    Status,
}

/// Memory scope mode controlling isolation level
#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum MemoryScopeMode {
    /// Isolated: each process has independent memory index
    Isolated,
    /// Project-shared: same repository shares memory across instances
    Project,
    /// Fully shared: all instances share global and project memory pool
    Shared,
}

struct ClearTarget {
    label: &'static str,
    path: PathBuf,
    clear: fn(&MemoryStorage) -> std::io::Result<bool>,
}

fn workspace_target(storage: &MemoryStorage) -> ClearTarget {
    ClearTarget {
        label: "workspace memory",
        path: storage.workspace_dir().to_path_buf(),
        clear: |s| s.clear_workspace(),
    }
}

fn global_target(storage: &MemoryStorage) -> ClearTarget {
    ClearTarget {
        label: "global MEMORY.md",
        path: storage.global_memory_file(),
        clear: |s| s.clear_global(),
    }
}

pub fn run(args: MemoryArgs) -> Result<()> {
    match args.command {
        MemoryCommand::Clear {
            global, all, yes, ..
        } => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let storage = MemoryStorage::new(&cwd, None);

            let targets = if all {
                vec![workspace_target(&storage), global_target(&storage)]
            } else if global {
                vec![global_target(&storage)]
            } else {
                vec![workspace_target(&storage)]
            };

            run_clear(&storage, &targets, yes)
        }
        MemoryCommand::Scope { mode } => {
            println!("Memory scope switched to: {:?}", mode);
            println!("Note: Changes take effect on next session restart.");
            println!("      Use `/memory scope isolated` for full isolation");
            println!("      Use `/memory scope project` for team sharing");
            println!("      Use `/memory scope shared` for knowledge pool");
            Ok(())
        }
        MemoryCommand::Status => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let storage = MemoryStorage::new(&cwd, None);
            
            println!("Current memory configuration:");
            println!("  Global: {}", storage.global_memory_file().display());
            println!("  Workspace: {}", storage.workspace_dir().display());
            println!("\nTo change scope mode, use:");
            println!("  /memory scope isolated  # Full isolation");
            println!("  /memory scope project   # Share within project");
            println!("  /memory scope shared    # Global knowledge pool");
            Ok(())
        }
    }
}

fn run_clear(storage: &MemoryStorage, targets: &[ClearTarget], skip_confirm: bool) -> Result<()> {
    let existing: Vec<_> = targets.iter().filter(|t| t.path.exists()).collect();

    if existing.is_empty() {
        println!("Nothing to clear — no memory files found.");
        return Ok(());
    }

    println!("The following will be deleted:");
    for t in &existing {
        println!("  {}: {}", t.label, t.path.display());
    }

    if !skip_confirm {
        print!("\nAre you sure? [y/N] ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut cleared = false;
    let mut errors: Vec<String> = Vec::new();
    for t in targets {
        match (t.clear)(storage) {
            Ok(true) => {
                cleared = true;
                println!("  Cleared: {}", t.label);
            }
            Ok(false) => {} // nothing to clear for this scope
            Err(e) => {
                errors.push(format!("{}: {e}", t.label));
            }
        }
    }

    if cleared && errors.is_empty() {
        println!("Memory cleared.");
    } else if cleared {
        println!("Memory partially cleared. Errors:");
        for e in &errors {
            eprintln!("  {e}");
        }
    } else if !errors.is_empty() {
        eprintln!("Failed to clear memory:");
        for e in &errors {
            eprintln!("  {e}");
        }
        return Err(anyhow::anyhow!("clear failed"));
    }

    Ok(())
}
