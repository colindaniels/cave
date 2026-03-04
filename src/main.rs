mod commands;
mod config;
mod ssh;
mod status;
mod vm;

use clap::{Parser, Subcommand};
use commands::{deploy, destroy, images, init, list, remove, server};

#[derive(Parser)]
#[command(name = "cave")]
#[command(about = "CLI for managing physical nodes with PXE boot capabilities")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize and manage the PXE server
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
    /// Register a new node
    Init {
        /// Hostname for the node
        hostname: String,
        /// IP address of the node
        ip: String,
        /// MAC address of the node
        mac: String,
    },
    /// List all registered nodes with status and specs
    List,
    /// Deploy an image as a VM on a node
    Deploy {
        /// Hostname of the node
        hostname: String,
        /// Image name to deploy
        image: String,
        /// Memory in MB (default: 2048)
        #[arg(long, default_value = "2048")]
        memory: u32,
        /// Number of CPUs (default: 2)
        #[arg(long, default_value = "2")]
        cpus: u32,
    },
    /// Stop and remove the VM on a node
    Destroy {
        /// Hostname of the node
        hostname: String,
    },
    /// Remove a node from the registry
    Remove {
        /// Hostname of the node
        hostname: String,
    },
    /// List local images
    Images,
    /// Image management commands
    Image {
        #[command(subcommand)]
        action: ImageAction,
    },
}

#[derive(Subcommand)]
enum ServerAction {
    /// Initialize the PXE server (download Alpine, generate SSH keys)
    Init {
        /// Port for the HTTP server
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Start the PXE server
    Start,
    /// Stop the PXE server
    Stop,
    /// Show server status
    Status,
    /// Tail server logs (Ctrl+C to exit)
    Logs,
}

#[derive(Subcommand)]
enum ImageAction {
    /// Download an image
    Pull {
        /// URL of the image to download
        url: String,
    },
    /// Search for images on netboot.xyz
    Search {
        /// Search query
        query: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { action } => match action {
            ServerAction::Init { port } => server::init(port).await?,
            ServerAction::Start => server::start().await?,
            ServerAction::Stop => server::stop().await?,
            ServerAction::Status => server::status().await?,
            ServerAction::Logs => server::logs().await?,
        },
        Commands::Init { hostname, ip, mac } => init::run(&hostname, &ip, &mac).await?,
        Commands::List => list::run().await?,
        Commands::Deploy { hostname, image, memory, cpus } => deploy::run(&hostname, &image, memory, cpus).await?,
        Commands::Destroy { hostname } => destroy::run(&hostname).await?,
        Commands::Remove { hostname } => remove::run(&hostname).await?,
        Commands::Images => images::list().await?,
        Commands::Image { action } => match action {
            ImageAction::Pull { url } => images::pull(&url).await?,
            ImageAction::Search { query } => images::search(&query).await?,
        },
    }

    Ok(())
}
