use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::{self, File};
use std::io::Write;
use std::net::UdpSocket;
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::ssh;

const ALPINE_VERSION: &str = "3.21";
const ALPINE_MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";

fn require_root(action: &str) -> Result<()> {
    // Check if running as root (UID 0)
    if unsafe { libc::geteuid() } != 0 {
        anyhow::bail!(
            "The 'cave server {}' command requires root privileges.\nRun with: sudo cave server {}",
            action, action
        );
    }
    Ok(())
}

pub async fn init(port: u16) -> Result<()> {
    println!("Initializing cave server...");

    // Create directory structure
    Config::ensure_dirs()?;
    println!("Created directory structure at {:?}", Config::cave_dir());

    // Generate SSH keypair
    ssh::generate_keypair()?;

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

    println!("\nServer initialized successfully!");
    println!("  - Alpine netboot files: {:?}", Config::alpine_dir());
    println!("  - SSH keys: {:?}", Config::ssh_dir());
    println!("  - HTTP port: {}", port);
    println!("\nRun 'cave server start' to start the PXE server.");

    Ok(())
}

async fn download_alpine_files() -> Result<()> {
    let alpine_dir = Config::alpine_dir();
    let flavor = "lts";

    let files = [
        (format!("vmlinuz-{}", flavor), format!("{}/v{}/releases/x86_64/netboot/vmlinuz-{}", ALPINE_MIRROR, ALPINE_VERSION, flavor)),
        (format!("initramfs-{}", flavor), format!("{}/v{}/releases/x86_64/netboot/initramfs-{}", ALPINE_MIRROR, ALPINE_VERSION, flavor)),
        (format!("modloop-{}", flavor), format!("{}/v{}/releases/x86_64/netboot/modloop-{}", ALPINE_MIRROR, ALPINE_VERSION, flavor)),
    ];

    for (filename, url) in &files {
        let filepath = alpine_dir.join(filename);
        if filepath.exists() {
            println!("  {} already exists, skipping", filename);
            continue;
        }

        println!("Downloading {}...", filename);
        download_file(&url, &filepath).await
            .with_context(|| format!("Failed to download {}", filename))?;
    }

    // Copy public key to alpine dir for serving
    let pub_key_src = Config::ssh_public_key();
    let pub_key_dst = alpine_dir.join("cave.pub");
    if pub_key_src.exists() {
        fs::copy(&pub_key_src, &pub_key_dst)
            .context("Failed to copy public key to alpine dir")?;
    }

    println!("Alpine netboot files downloaded successfully");
    Ok(())
}

async fn download_file(url: &str, filepath: &std::path::Path) -> Result<()> {
    let client = reqwest::Client::new();
    let response = client.get(url).send().await
        .with_context(|| format!("Failed to GET {}", url))?;

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut file = File::create(filepath)
        .with_context(|| format!("Failed to create file: {:?}", filepath))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read chunk")?;
        file.write_all(&chunk).context("Failed to write chunk")?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_with_message("done");
    Ok(())
}

fn find_pixiecore() -> Option<std::path::PathBuf> {
    // Check PATH first
    if let Ok(output) = Command::new("which").arg("pixiecore").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Some(std::path::PathBuf::from(path));
        }
    }

    // If running with sudo, check original user's ~/go/bin
    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        let go_bin = std::path::PathBuf::from(format!("/home/{}/go/bin/pixiecore", sudo_user));
        if go_bin.exists() {
            return Some(go_bin);
        }
    }

    // Check ~/go/bin
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
            println!("pixiecore found at: {:?}", path);
            Ok(())
        }
        None => {
            println!("\nWARNING: pixiecore not found in PATH or ~/go/bin");
            println!("Install it with: go install go.universe.tf/netboot/cmd/pixiecore@latest");
            println!("Or download from: https://github.com/danderson/netboot/releases");
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
        anyhow::bail!("Server not initialized. Run 'cave server init' first.");
    }

    let pid_file = Config::pixiecore_pid_file();
    if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?.trim().to_string();
        // Check if process is running
        let check = Command::new("kill")
            .args(["-0", &pid])
            .output();
        if check.map(|o| o.status.success()).unwrap_or(false) {
            println!("PXE server is already running (PID: {})", pid);
            return Ok(());
        }
        // Stale pid file, remove it
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

    // Build cmdline
    let cmdline = format!(
        "ip=dhcp ssh_key={} modloop={} alpine_repo={}",
        ssh_key_url, modloop_url, alpine_repo
    );

    println!("Starting PXE server...");
    println!("  Kernel: {:?}", vmlinuz);
    println!("  Initramfs: {:?}", initramfs);
    println!("  HTTP port: {}", port);
    println!("  Local IP: {}", local_ip);

    // Start simple HTTP server for serving files
    let http_handle = start_http_server(port)?;

    // Find pixiecore binary
    let pixiecore_path = find_pixiecore()
        .ok_or_else(|| anyhow::anyhow!("pixiecore not found. Install with: go install go.universe.tf/netboot/cmd/pixiecore@latest"))?;

    // Open log file for appending
    let log_file = Config::server_log_file();
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .with_context(|| format!("Failed to open log file: {:?}", log_file))?;
    let log_err = log.try_clone()?;

    // Start pixiecore (use port 8081 for pixiecore HTTP to avoid conflicts with port 80)
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

    let child = pixie_cmd.spawn()
        .with_context(|| format!("Failed to start pixiecore at {:?}", pixiecore_path))?;

    let pid = child.id();
    fs::write(&pid_file, format!("{}\n{}", pid, http_handle))
        .context("Failed to write PID file")?;

    println!("\nPXE server started!");
    println!("  pixiecore PID: {}", pid);
    println!("  HTTP server PID: {}", http_handle);
    println!("  Log file: {:?}", Config::server_log_file());
    println!("\nNodes will boot with SSH key from: {}", ssh_key_url);
    println!("Run 'cave server logs' to tail logs.");
    println!("Run 'cave server stop' to stop the server.");

    Ok(())
}

fn start_http_server(port: u16) -> Result<u32> {
    let alpine_dir = Config::alpine_dir();

    // Use Python's simple HTTP server
    let child = Command::new("python3")
        .args(["-m", "http.server", &port.to_string()])
        .current_dir(&alpine_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start HTTP server. Is python3 installed?")?;

    Ok(child.id())
}

pub async fn stop() -> Result<()> {
    require_root("stop")?;

    let pid_file = Config::pixiecore_pid_file();

    if !pid_file.exists() {
        println!("PXE server is not running");
        return Ok(());
    }

    let content = fs::read_to_string(&pid_file)?;
    let pids: Vec<&str> = content.lines().collect();

    for pid in pids {
        let pid = pid.trim();
        if !pid.is_empty() {
            let _ = Command::new("kill")
                .arg(pid)
                .output();
            println!("Stopped process: {}", pid);
        }
    }

    fs::remove_file(&pid_file)?;
    println!("PXE server stopped");

    Ok(())
}

pub async fn status() -> Result<()> {
    require_root("status")?;

    let config = Config::load()?;
    let pid_file = Config::pixiecore_pid_file();

    println!("Cave Server Status");
    println!("==================");
    println!("Initialized: {}", config.server.initialized);
    println!("HTTP Port: {}", config.server.port);
    println!("Alpine Version: {}", config.server.alpine_version);

    if pid_file.exists() {
        let content = fs::read_to_string(&pid_file)?;
        let pids: Vec<&str> = content.lines().collect();
        let mut running = false;

        for pid in &pids {
            let pid = pid.trim();
            if !pid.is_empty() {
                let check = Command::new("kill")
                    .args(["-0", pid])
                    .output();
                if check.map(|o| o.status.success()).unwrap_or(false) {
                    running = true;
                }
            }
        }

        if running {
            println!("Status: RUNNING");
            println!("PIDs: {}", pids.join(", "));
        } else {
            println!("Status: STOPPED (stale PID file)");
        }
    } else {
        println!("Status: STOPPED");
    }

    // Check for required files
    let alpine_dir = Config::alpine_dir();
    println!("\nAlpine Files:");
    for file in &["vmlinuz-lts", "initramfs-lts", "modloop-lts", "cave.pub"] {
        let path = alpine_dir.join(file);
        let status = if path.exists() { "OK" } else { "MISSING" };
        println!("  {}: {}", file, status);
    }

    Ok(())
}

pub async fn logs() -> Result<()> {
    let log_file = Config::server_log_file();

    if !log_file.exists() {
        println!("No log file found. Start the server first with 'cave server start'.");
        return Ok(());
    }

    println!("Tailing logs from {:?}", log_file);
    println!("Press Ctrl+C to exit.\n");

    // Use tail -f to follow the log file
    let mut child = Command::new("tail")
        .args(["-f", log_file.to_str().unwrap()])
        .spawn()
        .context("Failed to run tail command")?;

    // Wait for the child process (will be killed by Ctrl+C)
    let _ = child.wait();

    Ok(())
}
