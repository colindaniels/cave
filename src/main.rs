mod commands;
mod config;
mod ssh;
mod status;
mod tui;
mod ui;
mod vm;

use clap::{Parser, Subcommand};
use commands::{deploy, destroy, http_serve, images, init, list, poll, remove, server, shutdown, wake, watcher_start};

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
    /// Initialize the PXE server (download Alpine, generate SSH keys)
    Init {
        /// Port for the HTTP server
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Manage the PXE server
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
    /// Manage nodes
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    /// List local images
    Images,
    /// Image management commands
    Image {
        #[command(subcommand)]
        action: ImageAction,
    },
    /// Launch interactive TUI dashboard
    #[command(alias = "ui")]
    Tui,
    /// Internal: Start VM on a node (used by watcher)
    #[command(hide = true)]
    WatcherStart {
        /// Hostname of the node
        hostname: String,
    },
    /// Internal: HTTP file server (used by server start)
    #[command(hide = true)]
    HttpServe {
        /// Port to serve on
        port: u16,
        /// Directory to serve
        dir: String,
    },
    /// Internal: Background poll for IP cache and SSH config (used by watcher)
    #[command(hide = true)]
    Poll,
}

#[derive(Subcommand)]
enum ServerAction {
    /// Start the PXE server
    Start,
    /// Stop the PXE server
    Stop,
    /// Restart the PXE server
    Restart,
    /// Show server status
    Status,
    /// Tail server logs (Ctrl+C to exit)
    Logs,
}

#[derive(Subcommand)]
enum NodeAction {
    /// Register a new node
    Init {
        /// Hostname for the node
        hostname: String,
        /// MAC address of the node
        mac: String,
    },
    /// List all registered nodes with status and specs
    List,
    /// Deploy an image as a VM on a node
    Deploy {
        /// Hostname of the node (interactive if not provided)
        hostname: Option<String>,
        /// Image name to deploy (interactive if not provided)
        image: Option<String>,
    },
    /// Stop and remove the VM on a node
    Destroy {
        /// Hostname of the node
        hostname: String,
        /// VM name (defaults to node hostname if not specified)
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove a node from the registry
    Remove {
        /// Hostname of the node
        hostname: String,
    },
    /// Send Wake-on-LAN packet to power on a node
    Wake {
        /// Hostname of the node to wake
        hostname: String,
    },
    /// Gracefully shut down a node
    Shutdown {
        /// Hostname of the node to shut down
        hostname: String,
    },
    /// Restart a node (shutdown + wake)
    Restart {
        /// Hostname of the node to restart
        hostname: String,
    },
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
        Commands::Init { port } => server::init(port).await?,
        Commands::Server { action } => match action {
            ServerAction::Start => server::start().await?,
            ServerAction::Stop => server::stop().await?,
            ServerAction::Restart => {
                server::stop().await?;
                server::start().await?;
            }
            ServerAction::Status => server::status().await?,
            ServerAction::Logs => server::logs().await?,
        },
        Commands::Node { action } => match action {
            NodeAction::Init { hostname, mac } => init::run(&hostname, &mac).await?,
            NodeAction::List => list::run().await?,
            NodeAction::Deploy { hostname, image } => {
                deploy::run(hostname.as_deref(), image.as_deref()).await?
            }
            NodeAction::Destroy { hostname, name } => {
                let vm_name = name.as_deref().unwrap_or(&hostname);
                destroy::run(&hostname, vm_name).await?
            }
            NodeAction::Remove { hostname } => remove::run(&hostname).await?,
            NodeAction::Wake { hostname } => wake::run(&hostname).await?,
            NodeAction::Shutdown { hostname } => shutdown::run(&hostname).await?,
            NodeAction::Restart { hostname } => {
                shutdown::run(&hostname).await?;
                // Wait a moment for clean shutdown
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                wake::run(&hostname).await?;
            }
        },
        Commands::Images => images::list().await?,
        Commands::Image { action } => match action {
            ImageAction::Pull { url } => images::pull(&url).await?,
            ImageAction::Search { query } => images::search(&query).await?,
        },
        Commands::Tui => tui::run()?,
        Commands::WatcherStart { hostname } => watcher_start::run(&hostname).await?,
        Commands::HttpServe { port, dir } => http_serve::run(port, &dir).await?,
        Commands::Poll => poll::run().await?,
    }

    Ok(())
}
