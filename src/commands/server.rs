use anyhow::{Context, Result};
use console::style;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::{self, File};
use std::io::Write;
use std::net::UdpSocket;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::Config;
use crate::ssh;
use crate::ui;

const ALPINE_VERSION: &str = "3.21";
const ALPINE_MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";

fn require_root(action: &str) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        ui::print_error(&format!(
            "'cave server {}' requires root privileges",
            action
        ));
        println!(
            "  Run with: {}",
            style(format!("sudo cave server {}", action)).cyan()
        );
        std::process::exit(1);
    }
    Ok(())
}

pub async fn init(port: u16) -> Result<()> {
    ui::print_header("Initializing Cave Server");

    // Create directory structure
    let spinner = create_spinner("Creating directories...");
    Config::ensure_dirs()?;
    spinner.finish_and_clear();
    ui::print_success(&format!("Created {:?}", Config::cave_dir()));

    // Generate SSH keypair
    let spinner = create_spinner("Generating SSH keys...");
    ssh::generate_keypair()?;
    spinner.finish_and_clear();
    ui::print_success("SSH keys generated");

    // Download Alpine netboot files
    download_alpine_files().await?;

    // Save config
    let mut config = Config::load()?;
    config.server.port = port;
    config.server.alpine_version = ALPINE_VERSION.to_string();
    config.server.initialized = true;
    config.save()?;

    // Check if pixiecore is installed
    check_pixiecore()?;

    ui::print_completion("Server Initialized");
    println!();
    ui::print_box("Configuration", &[
        ("Alpine files", Config::alpine_dir().to_str().unwrap()),
        ("SSH keys", Config::ssh_dir().to_str().unwrap()),
        ("HTTP port", &port.to_string()),
    ]);
    println!();
    println!(
        "  {} {}",
        style("Next:").dim(),
        style("sudo cave server start").cyan()
    );

    Ok(())
}

async fn download_alpine_files() -> Result<()> {
    let alpine_dir = Config::alpine_dir();
    let flavor = "lts";

    let files = [
        (
            format!("vmlinuz-{}", flavor),
            format!(
                "{}/v{}/releases/x86_64/netboot/vmlinuz-{}",
                ALPINE_MIRROR, ALPINE_VERSION, flavor
            ),
        ),
        (
            format!("initramfs-{}", flavor),
            format!(
                "{}/v{}/releases/x86_64/netboot/initramfs-{}",
                ALPINE_MIRROR, ALPINE_VERSION, flavor
            ),
        ),
        (
            format!("modloop-{}", flavor),
            format!(
                "{}/v{}/releases/x86_64/netboot/modloop-{}",
                ALPINE_MIRROR, ALPINE_VERSION, flavor
            ),
        ),
    ];

    for (filename, url) in &files {
        let filepath = alpine_dir.join(filename);
        if filepath.exists() {
            ui::print_info(&format!("{} exists, skipping", filename));
            continue;
        }

        println!(
            "{} Downloading {}...",
            style("→").cyan().bold(),
            style(filename).bold()
        );
        download_file(url, &filepath).await.with_context(|| format!("Failed to download {}", filename))?;
    }

    // Copy public key to alpine dir for serving
    let pub_key_src = Config::ssh_public_key();
    let pub_key_dst = alpine_dir.join("cave.pub");
    if pub_key_src.exists() {
        fs::copy(&pub_key_src, &pub_key_dst).context("Failed to copy public key")?;
    }

    ui::print_success("Alpine netboot files ready");
    Ok(())
}

async fn download_file(url: &str, filepath: &std::path::Path) -> Result<()> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to GET {}", url))?;

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("━━╸"),
    );

    let mut file =
        File::create(filepath).with_context(|| format!("Failed to create file: {:?}", filepath))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read chunk")?;
        file.write_all(&chunk).context("Failed to write chunk")?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_and_clear();
    Ok(())
}

fn find_pixiecore() -> Option<std::path::PathBuf> {
    if let Ok(output) = Command::new("which").arg("pixiecore").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Some(std::path::PathBuf::from(path));
        }
    }

    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        let go_bin = std::path::PathBuf::from(format!("/home/{}/go/bin/pixiecore", sudo_user));
        if go_bin.exists() {
            return Some(go_bin);
        }
    }

    if let Some(home) = dirs::home_dir() {
        let go_bin = home.join("go").join("bin").join("pixiecore");
        if go_bin.exists() {
            return Some(go_bin);
        }
    }

    None
}

fn check_pixiecore() -> Result<()> {
    match find_pixiecore() {
        Some(path) => {
            ui::print_success(&format!("pixiecore found at {:?}", path));
            Ok(())
        }
        None => {
            ui::print_warning("pixiecore not found");
            println!(
                "  Install: {}",
                style("go install go.universe.tf/netboot/cmd/pixiecore@latest").cyan()
            );
            Ok(())
        }
    }
}

fn get_local_ip() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    let local_addr = socket.local_addr()?;
    Ok(local_addr.ip().to_string())
}

pub async fn start() -> Result<()> {
    require_root("start")?;

    let config = Config::load()?;

    if !config.server.initialized {
        ui::print_error("Server not initialized");
        println!("  Run {} first", style("cave server init").cyan());
        return Ok(());
    }

    let pid_file = Config::pixiecore_pid_file();
    if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?.trim().to_string();
        let check = Command::new("kill").args(["-0", &pid]).output();
        if check.map(|o| o.status.success()).unwrap_or(false) {
            ui::print_warning(&format!("PXE server already running (PID: {})", pid));
            return Ok(());
        }
        fs::remove_file(&pid_file)?;
    }

    let alpine_dir = Config::alpine_dir();
    let port = config.server.port;
    let local_ip = get_local_ip()?;

    let vmlinuz = alpine_dir.join("vmlinuz-lts");
    let initramfs = alpine_dir.join("initramfs-lts");
    let modloop_url = format!("http://{}:{}/modloop-lts", local_ip, port);
    let ssh_key_url = format!("http://{}:{}/cave.pub", local_ip, port);
    let alpine_repo = format!("{}/v{}/main", ALPINE_MIRROR, config.server.alpine_version);

    let cmdline = format!(
        "ip=dhcp ssh_key={} modloop={} alpine_repo={}",
        ssh_key_url, modloop_url, alpine_repo
    );

    ui::print_header("Starting PXE Server");

    // Start HTTP server
    let spinner = create_spinner("Starting HTTP server...");
    let http_handle = start_http_server(port)?;
    spinner.finish_and_clear();
    ui::print_success(&format!("HTTP server on port {}", port));

    // Find pixiecore
    let pixiecore_path = find_pixiecore().ok_or_else(|| {
        anyhow::anyhow!(
            "pixiecore not found. Install: {}",
            style("go install go.universe.tf/netboot/cmd/pixiecore@latest").cyan()
        )
    })?;

    // Start pixiecore
    let spinner = create_spinner("Starting pixiecore...");

    let log_file = Config::server_log_file();
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .with_context(|| format!("Failed to open log file: {:?}", log_file))?;
    let log_err = log.try_clone()?;

    let pixie_http_port = "8081";
    let mut pixie_cmd = Command::new(&pixiecore_path);
    pixie_cmd
        .arg("boot")
        .arg(&vmlinuz)
        .arg(&initramfs)
        .arg("--cmdline")
        .arg(&cmdline)
        .arg("--dhcp-no-bind")
        .arg("--port")
        .arg(pixie_http_port)
        .arg("--debug")
        .stdout(log)
        .stderr(log_err);

    let child = pixie_cmd
        .spawn()
        .with_context(|| format!("Failed to start pixiecore at {:?}", pixiecore_path))?;

    let pid = child.id();
    fs::write(&pid_file, format!("{}\n{}", pid, http_handle))
        .context("Failed to write PID file")?;

    spinner.finish_and_clear();
    ui::print_success("pixiecore started");

    // Start VM watcher in background
    let spinner = create_spinner("Starting VM watcher...");
    let watcher_pid = start_vm_watcher()?;
    spinner.finish_and_clear();
    ui::print_success("VM watcher started");

    // Save all PIDs
    fs::write(&pid_file, format!("{}\n{}\n{}", pid, http_handle, watcher_pid))
        .context("Failed to write PID file")?;

    ui::print_completion("PXE Server Running");
    println!();
    ui::print_box("Server Info", &[
        ("Local IP", &local_ip),
        ("HTTP port", &port.to_string()),
        ("pixiecore PID", &pid.to_string()),
        ("VM watcher PID", &watcher_pid.to_string()),
    ]);
    println!();
    println!(
        "  {} {}",
        style("Logs:").dim(),
        style("cave server logs").cyan()
    );
    println!(
        "  {} {}",
        style("Stop:").dim(),
        style("sudo cave server stop").cyan()
    );

    Ok(())
}

fn start_http_server(port: u16) -> Result<u32> {
    let alpine_dir = Config::alpine_dir();

    // Get path to cave binary
    let cave_bin = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("cave"));

    let child = Command::new(&cave_bin)
        .args([
            "http-serve",
            &port.to_string(),
            alpine_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start HTTP server")?;

    Ok(child.id())
}

/// Start background VM watcher that auto-starts VMs on standby nodes
fn start_vm_watcher() -> Result<u32> {
    let cave_dir = Config::cave_dir();
    let vms_dir = Config::vms_dir();
    let watcher_log = cave_dir.join("watcher.log");

    // Get path to cave binary
    let cave_bin = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("cave"));

    // Simple watcher that calls `cave poll` and `cave watcher-start <hostname>` for each config
    // This uses the exact same code path as deploy
    let watcher_script = format!(
        r#"#!/bin/sh
# Cave VM Watcher - auto-starts VMs, updates IP cache and SSH config
CAVE="{cave_bin}"
VMS_DIR="{vms_dir}"
LOG="{log}"

log() {{
    echo "$(date '+%Y-%m-%d %H:%M:%S') [watcher] $1" >> "$LOG"
}}

log "Watcher started, configs in $VMS_DIR"

while true; do
    # Poll network to update IP cache and SSH config
    "$CAVE" poll 2>/dev/null

    for conf in "$VMS_DIR"/*.conf; do
        [ -f "$conf" ] || continue
        hostname=$(basename "$conf" .conf)
        # Call cave watcher-start which uses the same code as deploy
        if "$CAVE" watcher-start "$hostname" 2>/dev/null; then
            log "Started VM on $hostname"
        fi
    done
    sleep 10
done
"#,
        cave_bin = cave_bin.display(),
        vms_dir = vms_dir.display(),
        log = watcher_log.display()
    );

    // Write watcher script
    let script_path = cave_dir.join("watcher.sh");
    fs::write(&script_path, &watcher_script)?;

    // Make executable and run
    let child = Command::new("sh")
        .arg(&script_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start VM watcher")?;

    Ok(child.id())
}

pub async fn stop() -> Result<()> {
    require_root("stop")?;

    let pid_file = Config::pixiecore_pid_file();

    if !pid_file.exists() {
        ui::print_warning("PXE server is not running");
        return Ok(());
    }

    let spinner = create_spinner("Stopping server...");

    let content = fs::read_to_string(&pid_file)?;
    let pids: Vec<&str> = content.lines().collect();

    for pid in pids {
        let pid = pid.trim();
        if !pid.is_empty() {
            let _ = Command::new("kill").arg(pid).output();
        }
    }

    fs::remove_file(&pid_file)?;
    spinner.finish_and_clear();

    ui::print_success("PXE server stopped");

    Ok(())
}

pub async fn status() -> Result<()> {
    require_root("status")?;

    let config = Config::load()?;
    let pid_file = Config::pixiecore_pid_file();

    ui::print_header("Server Status");
    println!();

    // Check running status
    let running = if pid_file.exists() {
        let content = fs::read_to_string(&pid_file)?;
        let pids: Vec<&str> = content.lines().collect();
        let mut any_running = false;

        for pid in &pids {
            let pid = pid.trim();
            if !pid.is_empty() {
                let check = Command::new("kill").args(["-0", pid]).output();
                if check.map(|o| o.status.success()).unwrap_or(false) {
                    any_running = true;
                }
            }
        }
        any_running
    } else {
        false
    };

    println!(
        "  {} {}",
        style("Status:").dim(),
        if running {
            style("RUNNING").green().bold()
        } else {
            style("STOPPED").red()
        }
    );
    println!(
        "  {} {}",
        style("Initialized:").dim(),
        if config.server.initialized {
            style("Yes").green()
        } else {
            style("No").red()
        }
    );
    println!(
        "  {} {}",
        style("HTTP Port:").dim(),
        config.server.port
    );
    println!(
        "  {} {}",
        style("Alpine:").dim(),
        config.server.alpine_version
    );

    // Check files
    println!();
    println!("  {}", style("Files:").dim());
    let alpine_dir = Config::alpine_dir();
    for file in &["vmlinuz-lts", "initramfs-lts", "modloop-lts", "cave.pub"] {
        let path = alpine_dir.join(file);
        let status = if path.exists() {
            style("OK").green().to_string()
        } else {
            style("MISSING").red().to_string()
        };
        println!("    {} {}", status, file);
    }

    Ok(())
}

pub async fn logs() -> Result<()> {
    let log_file = Config::server_log_file();

    if !log_file.exists() {
        ui::print_warning("No log file found");
        println!(
            "  Start server first: {}",
            style("sudo cave server start").cyan()
        );
        return Ok(());
    }

    println!(
        "{} Tailing {} {}",
        style("→").cyan().bold(),
        style(log_file.to_str().unwrap()).dim(),
        style("(Ctrl+C to exit)").dim()
    );
    println!();

    let mut child = Command::new("tail")
        .args(["-f", log_file.to_str().unwrap()])
        .spawn()
        .context("Failed to run tail command")?;

    let _ = child.wait();

    Ok(())
}

fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}
