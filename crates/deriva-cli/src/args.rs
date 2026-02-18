use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "deriva", about = "Deriva command-line client")]
pub struct Cli {
    /// Server address
    #[arg(long, default_value = "http://[::1]:50051", global = true)]
    pub server: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Store leaf data from a file or stdin
    Put {
        /// File path, or "-" for stdin
        source: String,
    },
    /// Register a recipe
    Recipe {
        /// Function name
        function: String,
        /// Function version
        version: String,
        /// Input CAddrs (hex)
        #[arg(short, long, required = true)]
        input: Vec<String>,
        /// Parameters as key=value
        #[arg(short, long)]
        param: Vec<String>,
    },
    /// Materialize a CAddr
    Get {
        /// CAddr (hex)
        addr: String,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Show the recipe behind a CAddr
    Resolve {
        /// CAddr (hex)
        addr: String,
    },
    /// Invalidate a cached entry
    Invalidate {
        /// CAddr (hex)
        addr: String,
        /// Cascade to all transitive dependents
        #[arg(long)]
        cascade: bool,
        /// Dry run: show what would be evicted
        #[arg(long)]
        dry_run: bool,
        /// Show addresses of evicted entries
        #[arg(long)]
        detail: bool,
    },
    /// Server status
    Status,
}
