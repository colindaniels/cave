use anyhow::{Context, Result};
use ssh2::Session;
use std::fs;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::config::Config;

pub fn generate_keypair() -> Result<()> {
    let private_key = Config::ssh_private_key();
    let public_key = Config::ssh_public_key();

    if private_key.exists() && public_key.exists() {
        println!("SSH keypair already exists");
        return Ok(());
    }

    Config::ensure_dirs()?;

    println!("Generating SSH keypair...");
    let output = Command::new("ssh-keygen")
        .args([
            "-t", "ed25519",
            "-f", private_key.to_str().unwrap(),
            "-N", "",
            "-C", "cave@localhost",
        ])
        .output()
        .context("Failed to run ssh-keygen")?;

    if !output.status.success() {
        anyhow::bail!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Set proper permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&private_key, fs::Permissions::from_mode(0o600))
            .context("Failed to set private key permissions")?;
    }

    println!("SSH keypair generated at {:?}", Config::ssh_dir());
    Ok(())
}

/// Get path to cave's SSH config file
pub fn cave_ssh_config_path() -> std::path::PathBuf {
    Config::ssh_dir().join("config")
}

/// Ensure ~/.ssh/config includes cave's SSH config
pub fn setup_ssh_include() -> Result<()> {
    let user_ssh_config = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".ssh")
        .join("config");

    let cave_config = cave_ssh_config_path();
    let include_line = format!("Include {}", cave_config.display());

    // Ensure ~/.ssh directory exists
    if let Some(parent) = user_ssh_config.parent() {
        fs::create_dir_all(parent).context("Failed to create .ssh directory")?;
    }

    // Ensure cave ssh directory exists
    Config::ensure_dirs()?;

    // Create empty cave config if it doesn't exist
    if !cave_config.exists() {
        fs::write(&cave_config, "# Cave managed SSH hosts\n")
            .context("Failed to create cave SSH config")?;
    }

    // Read existing user config
    let existing = if user_ssh_config.exists() {
        fs::read_to_string(&user_ssh_config)
            .context("Failed to read SSH config")?
    } else {
        String::new()
    };

    // Check if Include is already present
    if existing.contains(&include_line) {
        return Ok(());
    }

    // Add Include at the top of the file
    let new_content = format!("{}\n\n{}", include_line, existing);
    fs::write(&user_ssh_config, new_content)
        .context("Failed to write SSH config")?;

    Ok(())
}

pub fn update_ssh_config(hostname: &str, ip: &str) -> Result<()> {
    let ssh_config_path = cave_ssh_config_path();
    let private_key = Config::ssh_private_key();

    let host_entry = format!(
        "\n# Cave managed node: {}\nHost {}\n    HostName {}\n    User root\n    IdentityFile {}\n    StrictHostKeyChecking no\n    UserKnownHostsFile /dev/null\n",
        hostname, hostname, ip, private_key.display()
    );

    // Ensure cave ssh directory exists
    Config::ensure_dirs()?;

    // Read existing config
    let existing = if ssh_config_path.exists() {
        fs::read_to_string(&ssh_config_path)
            .context("Failed to read cave SSH config")?
    } else {
        "# Cave managed SSH hosts\n".to_string()
    };

    // Remove existing entry if present, then add new one
    let cleaned = remove_cave_entry(&existing, hostname);
    let new_content = format!("{}{}", cleaned, host_entry);
    fs::write(&ssh_config_path, new_content)
        .context("Failed to write cave SSH config")?;

    Ok(())
}

fn remove_cave_entry(content: &str, hostname: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut skip_until_next_host = false;

    for line in &lines {
        if line.starts_with("# Cave managed node:") && line.contains(hostname) {
            skip_until_next_host = true;
            continue;
        }
        if skip_until_next_host {
            if (line.starts_with("Host ") || line.starts_with("# ")) && !line.contains(hostname) {
                skip_until_next_host = false;
            } else if line.trim().is_empty() {
                continue;
            } else {
                continue;
            }
        }
        if !skip_until_next_host {
            new_lines.push(line);
        }
    }

    new_lines.join("\n")
}

pub fn remove_ssh_config(hostname: &str) -> Result<()> {
    let ssh_config_path = cave_ssh_config_path();

    if !ssh_config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&ssh_config_path)
        .context("Failed to read cave SSH config")?;

    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut skip_until_next_host = false;
    let mut found = false;

    for line in &lines {
        if line.starts_with("# Cave managed node:") && line.contains(hostname) {
            skip_until_next_host = true;
            found = true;
            continue;
        }
        if skip_until_next_host {
            if line.starts_with("Host ") && !line.contains(hostname) {
                skip_until_next_host = false;
            } else if line.starts_with("#") && !line.contains(hostname) {
                skip_until_next_host = false;
            } else {
                continue;
            }
        }
        new_lines.push(line);
    }

    if found {
        let new_content = new_lines.join("\n");
        fs::write(&ssh_config_path, new_content)
            .context("Failed to write cave SSH config")?;
    }

    Ok(())
}

pub struct SshConnection {
    session: Session,
}

impl SshConnection {
    pub fn connect(ip: &str) -> Result<Self> {
        Self::connect_timeout(ip, Duration::from_secs(10))
    }

    /// Connect with a custom timeout - useful for quick status checks
    pub fn connect_timeout(ip: &str, timeout: Duration) -> Result<Self> {
        let private_key = Config::ssh_private_key();

        let addr = format!("{}:22", ip)
            .to_socket_addrs()
            .with_context(|| format!("Invalid address: {}", ip))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", ip))?;

        let tcp = TcpStream::connect_timeout(&addr, timeout)
            .with_context(|| format!("Failed to connect to {}:22", ip))?;

        // Set read/write timeouts too
        tcp.set_read_timeout(Some(timeout)).ok();
        tcp.set_write_timeout(Some(timeout)).ok();

        let mut session = Session::new()
            .context("Failed to create SSH session")?;

        session.set_timeout(timeout.as_millis() as u32);
        session.set_tcp_stream(tcp);
        session.handshake()
            .context("SSH handshake failed")?;

        session
            .userauth_pubkey_file("root", None, &private_key, None)
            .context("SSH authentication failed")?;

        Ok(Self { session })
    }

    /// Quick check if we can connect (2 second timeout)
    pub fn can_connect_fast(ip: &str) -> bool {
        Self::connect_timeout(ip, Duration::from_secs(2)).is_ok()
    }

    pub fn execute(&self, command: &str) -> Result<String> {
        let mut channel = self.session.channel_session()
            .context("Failed to open SSH channel")?;

        channel.exec(command)
            .with_context(|| format!("Failed to execute command: {}", command))?;

        let mut output = String::new();
        channel.read_to_string(&mut output)
            .context("Failed to read command output")?;

        channel.wait_close()
            .context("Failed to close channel")?;

        Ok(output)
    }

    pub fn execute_with_status(&self, command: &str) -> Result<(String, i32)> {
        let mut channel = self.session.channel_session()
            .context("Failed to open SSH channel")?;

        channel.exec(command)
            .with_context(|| format!("Failed to execute command: {}", command))?;

        let mut output = String::new();
        channel.read_to_string(&mut output)
            .context("Failed to read command output")?;

        channel.wait_close()
            .context("Failed to close channel")?;

        let exit_status = channel.exit_status()
            .unwrap_or(-1);

        Ok((output, exit_status))
    }

    pub fn is_connected(&self) -> bool {
        self.session.authenticated()
    }
}

pub fn can_connect(ip: &str) -> bool {
    SshConnection::connect(ip).is_ok()
}

/// Enable Wake-on-LAN on the node's network interface
/// This must be called after boot for WoL to work on shutdown
pub fn enable_wol(ssh: &SshConnection) -> Result<()> {
    // Install ethtool if needed and enable WoL on eth0
    let cmd = r#"
        if ! which ethtool >/dev/null 2>&1; then
            apk add --no-cache ethtool >/dev/null 2>&1
        fi
        # Find the main ethernet interface (not lo, not br*, not tap*)
        iface=$(ip -o link show | awk -F': ' '$2 !~ /^(lo|br|tap|veth)/ {print $2; exit}')
        if [ -n "$iface" ]; then
            ethtool -s "$iface" wol g 2>/dev/null || true
        fi
    "#;
    ssh.execute(cmd)?;
    Ok(())
}

pub fn scp_file(ip: &str, local_path: &Path, remote_path: &str) -> Result<()> {
    let private_key = Config::ssh_private_key();

    let output = Command::new("scp")
        .args([
            "-i", private_key.to_str().unwrap(),
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            local_path.to_str().unwrap(),
            &format!("root@{}:{}", ip, remote_path),
        ])
        .output()
        .context("Failed to run scp")?;

    if !output.status.success() {
        anyhow::bail!(
            "scp failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
