use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::{self, File};
use std::io::Write;
use tabled::{Table, Tabled};

use crate::config::Config;

#[derive(Tabled)]
struct ImageRow {
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "SIZE")]
    size: String,
}

pub async fn list() -> Result<()> {
    let images_dir = Config::images_dir();

    if !images_dir.exists() {
        println!("No images directory. Run 'cave server init' first.");
        return Ok(());
    }

    let entries = fs::read_dir(&images_dir)
        .context("Failed to read images directory")?;

    let mut rows: Vec<ImageRow> = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let metadata = fs::metadata(&path)?;
            let size = format_size(metadata.len());

            rows.push(ImageRow { name, size });
        }
    }

    if rows.is_empty() {
        println!("No images found in {:?}", images_dir);
        println!("Use 'cave image pull <url>' to download an image.");
        return Ok(());
    }

    let table = Table::new(rows).to_string();
    println!("{}", table);

    Ok(())
}

pub async fn pull(url: &str) -> Result<()> {
    Config::ensure_dirs()?;

    let images_dir = Config::images_dir();

    // Extract filename from URL
    let filename = url
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Could not extract filename from URL"))?;

    let filepath = images_dir.join(filename);

    if filepath.exists() {
        println!("Image '{}' already exists. Overwriting...", filename);
    }

    println!("Downloading {}...", filename);

    let client = reqwest::Client::new();
    let response = client.get(url).send().await
        .with_context(|| format!("Failed to GET {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download: HTTP {}", response.status());
    }

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut file = File::create(&filepath)
        .with_context(|| format!("Failed to create file: {:?}", filepath))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read chunk")?;
        file.write_all(&chunk).context("Failed to write chunk")?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_with_message("done");

    println!("\nImage downloaded to {:?}", filepath);

    Ok(())
}

pub async fn search(query: &str) -> Result<()> {
    println!("Searching netboot.xyz for '{}'...\n", query);

    // Fetch the netboot.xyz endpoints.yml or menu
    let url = "https://raw.githubusercontent.com/netbootxyz/netboot.xyz/development/endpoints.yml";

    let client = reqwest::Client::new();
    let response = client.get(url).send().await
        .context("Failed to fetch netboot.xyz endpoints")?;

    if !response.status().is_success() {
        // Fallback to showing common distros
        println!("Could not fetch netboot.xyz index. Here are common image sources:\n");
        print_common_images(query);
        return Ok(());
    }

    let content = response.text().await?;

    // Simple search through the content
    let query_lower = query.to_lowercase();
    let mut found = false;

    println!("Matching entries from netboot.xyz:\n");

    for line in content.lines() {
        if line.to_lowercase().contains(&query_lower) {
            println!("  {}", line.trim());
            found = true;
        }
    }

    if !found {
        println!("No exact matches found. Here are common image sources:\n");
        print_common_images(query);
    }

    println!("\nTo download an image, use:");
    println!("  cave image pull <url>");

    Ok(())
}

fn print_common_images(query: &str) {
    let common = vec![
        ("Ubuntu Server", "https://releases.ubuntu.com/22.04/ubuntu-22.04.4-live-server-amd64.iso"),
        ("Debian", "https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-12.5.0-amd64-netinst.iso"),
        ("Alpine Linux", "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-standard-3.21.0-x86_64.iso"),
        ("Arch Linux", "https://mirror.rackspace.com/archlinux/iso/latest/archlinux-x86_64.iso"),
        ("Fedora Server", "https://download.fedoraproject.org/pub/fedora/linux/releases/40/Server/x86_64/iso/Fedora-Server-dvd-x86_64-40-1.14.iso"),
        ("Rocky Linux", "https://download.rockylinux.org/pub/rocky/9/isos/x86_64/Rocky-9.3-x86_64-minimal.iso"),
        ("NixOS", "https://channels.nixos.org/nixos-24.05/latest-nixos-minimal-x86_64-linux.iso"),
    ];

    let query_lower = query.to_lowercase();

    let mut matched = false;
    for (name, url) in &common {
        if name.to_lowercase().contains(&query_lower) || query_lower.is_empty() {
            println!("  {} ", name);
            println!("    {}\n", url);
            matched = true;
        }
    }

    if !matched {
        // Show all if no match
        for (name, url) in &common {
            println!("  {}", name);
            println!("    {}\n", url);
        }
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
