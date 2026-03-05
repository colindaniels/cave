use anyhow::{Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::{self, File};
use std::io::Write;

use crate::config::Config;
use crate::ui;

pub async fn list() -> Result<()> {
    let images_dir = Config::images_dir();

    if !images_dir.exists() {
        ui::print_warning("No images directory");
        println!("  Run {} first", style("cave server init").cyan());
        return Ok(());
    }

    let entries = fs::read_dir(&images_dir).context("Failed to read images directory")?;

    let mut images: Vec<(String, u64)> = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Skip seed ISOs
            if name.ends_with("-seed.iso") || name.ends_with("-seed") {
                continue;
            }

            let metadata = fs::metadata(&path)?;
            images.push((name, metadata.len()));
        }
    }

    if images.is_empty() {
        ui::print_warning("No images found");
        println!(
            "  Download one with: {}",
            style("cave image pull <url>").cyan()
        );
        return Ok(());
    }

    // Sort by name
    images.sort_by(|a, b| a.0.cmp(&b.0));

    println!();
    println!(
        "  {}",
        style(format!("{:<50} {:>10}", "IMAGE", "SIZE")).dim()
    );
    println!("  {}", style("─".repeat(62)).dim());

    for (name, size) in &images {
        println!(
            "  {:<50} {:>10}",
            style(name).bold(),
            style(ui::format_size(*size)).dim()
        );
    }

    println!();
    println!(
        "  {} {}",
        style(format!("{} images", images.len())).dim(),
        style(format!("in {:?}", images_dir)).dim()
    );

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
        ui::print_warning(&format!("'{}' already exists, overwriting", filename));
    }

    println!(
        "\n{} Downloading {}",
        style("→").cyan().bold(),
        style(filename).bold()
    );

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to GET {}", url))?;

    if !response.status().is_success() {
        ui::print_error(&format!("HTTP {}", response.status()));
        return Ok(());
    }

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("━━╸"),
    );

    let mut file =
        File::create(&filepath).with_context(|| format!("Failed to create file: {:?}", filepath))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read chunk")?;
        file.write_all(&chunk).context("Failed to write chunk")?;
        pb.inc(chunk.len() as u64);
    }

    pb.finish_and_clear();

    ui::print_success(&format!("Downloaded {}", filename));
    println!("  {} {:?}", style("Location:").dim(), filepath);

    Ok(())
}

struct CloudImage {
    name: &'static str,
    version: &'static str,
    arch: &'static str,
    format: &'static str,
    size: &'static str,
    url: &'static str,
}

/// Get a friendly display name for an image file
/// Returns the friendly name if it matches a known cloud image, otherwise returns the filename
pub fn get_image_display_name(filename: &str) -> String {
    for img in CLOUD_IMAGES {
        // Extract filename from URL
        if let Some(url_filename) = img.url.rsplit('/').next() {
            if filename == url_filename {
                return format!("{} {} ({}, {})", img.name, img.version, img.arch, img.size);
            }
        }
    }
    // Not a known cloud image, return filename as-is
    filename.to_string()
}

const CLOUD_IMAGES: &[CloudImage] = &[
    // Ubuntu
    CloudImage {
        name: "Ubuntu",
        version: "24.04 LTS",
        arch: "amd64",
        format: "img",
        size: "~700 MB",
        url: "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img",
    },
    CloudImage {
        name: "Ubuntu",
        version: "22.04 LTS",
        arch: "amd64",
        format: "img",
        size: "~650 MB",
        url: "https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img",
    },
    CloudImage {
        name: "Ubuntu",
        version: "20.04 LTS",
        arch: "amd64",
        format: "img",
        size: "~550 MB",
        url: "https://cloud-images.ubuntu.com/focal/current/focal-server-cloudimg-amd64.img",
    },
    // Debian
    CloudImage {
        name: "Debian",
        version: "12 (Bookworm)",
        arch: "amd64",
        format: "qcow2",
        size: "~350 MB",
        url: "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-generic-amd64.qcow2",
    },
    CloudImage {
        name: "Debian",
        version: "11 (Bullseye)",
        arch: "amd64",
        format: "qcow2",
        size: "~300 MB",
        url: "https://cloud.debian.org/images/cloud/bullseye/latest/debian-11-generic-amd64.qcow2",
    },
    // Fedora
    CloudImage {
        name: "Fedora",
        version: "41",
        arch: "amd64",
        format: "qcow2",
        size: "~450 MB",
        url: "https://download.fedoraproject.org/pub/fedora/linux/releases/41/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-41-1.4.x86_64.qcow2",
    },
    CloudImage {
        name: "Fedora",
        version: "40",
        arch: "amd64",
        format: "qcow2",
        size: "~450 MB",
        url: "https://download.fedoraproject.org/pub/fedora/linux/releases/40/Cloud/x86_64/images/Fedora-Cloud-Base-Generic.x86_64-40-1.14.qcow2",
    },
    // Alma / Rocky
    CloudImage {
        name: "AlmaLinux",
        version: "9",
        arch: "amd64",
        format: "qcow2",
        size: "~600 MB",
        url: "https://repo.almalinux.org/almalinux/9/cloud/x86_64/images/AlmaLinux-9-GenericCloud-latest.x86_64.qcow2",
    },
    CloudImage {
        name: "Rocky Linux",
        version: "9",
        arch: "amd64",
        format: "qcow2",
        size: "~600 MB",
        url: "https://download.rockylinux.org/pub/rocky/9/images/x86_64/Rocky-9-GenericCloud.latest.x86_64.qcow2",
    },
    // Arch
    CloudImage {
        name: "Arch Linux",
        version: "latest",
        arch: "amd64",
        format: "qcow2",
        size: "~500 MB",
        url: "https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2",
    },
    // Alpine
    CloudImage {
        name: "Alpine",
        version: "3.21",
        arch: "amd64",
        format: "qcow2",
        size: "~150 MB",
        url: "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/cloud/nocloud_alpine-3.21.3-x86_64-bios-cloudinit-r0.qcow2",
    },
];

pub async fn search(query: &str) -> Result<()> {
    let query_lower = query.to_lowercase();
    let theme = ColorfulTheme::default();

    // Filter images matching the query
    let matches: Vec<&CloudImage> = CLOUD_IMAGES
        .iter()
        .filter(|img| {
            img.name.to_lowercase().contains(&query_lower)
                || img.version.to_lowercase().contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        ui::print_warning(&format!("No images matching '{}'", query));
        println!();
        println!("  Available: Ubuntu, Debian, Fedora, AlmaLinux, Rocky, Arch, Alpine");
        return Ok(());
    }

    println!();
    println!(
        "  {}",
        style(format!("Found {} images matching '{}'", matches.len(), query)).dim()
    );
    println!();

    // Build selection list
    let options: Vec<String> = matches
        .iter()
        .map(|img| {
            format!(
                "{} {} ({}, {})",
                img.name, img.version, img.arch, img.size
            )
        })
        .collect();

    let selection = FuzzySelect::with_theme(&theme)
        .with_prompt("Select image to download")
        .items(&options)
        .default(0)
        .interact_opt()?;

    match selection {
        Some(idx) => {
            let img = matches[idx];
            println!();
            pull(img.url).await?;
        }
        None => {
            println!("{}", style("Cancelled").dim());
        }
    }

    Ok(())
}
