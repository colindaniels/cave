use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::commands::images::{CloudImage, CLOUD_IMAGES};
use crate::config::{load_node_cache, CachedNode, Config};

use super::ui;

// ============================================================================
// Overlay Screens (popups over the main view)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NodeActions,  // Action menu for selected node
    Deploy(DeployStep),
    ImageDownload,
    NodeInit,
    Help,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeployStep {
    SelectImage,
    SelectDisk,
    Configure,
    Confirm,
    Deploying,
    Done,
}

// ============================================================================
// Constants
// ============================================================================

pub const MEMORY_OPTIONS: &[(u64, &str)] = &[
    (1024, "1 GB"),
    (2048, "2 GB"),
    (4096, "4 GB"),
    (8192, "8 GB"),
    (16384, "16 GB"),
    (32768, "32 GB"),
];

pub const CPU_OPTIONS: &[(u32, &str)] = &[
    (1, "1 CPU"),
    (2, "2 CPUs"),
    (4, "4 CPUs"),
    (8, "8 CPUs"),
    (16, "16 CPUs"),
];

pub const DISK_OPTIONS: &[(u64, &str)] = &[
    (10, "10 GB"),
    (20, "20 GB"),
    (50, "50 GB"),
    (100, "100 GB"),
    (200, "200 GB"),
    (500, "500 GB"),
];

pub const NODE_ACTIONS: &[&str] = &[
    "Deploy VM",
    "Destroy VM",
    "Wake (WoL)",
    "Shutdown",
    "Restart",
    "Remove Node",
];

// ============================================================================
// App State
// ============================================================================

pub struct App {
    // Core state
    pub running: bool,
    pub overlay: Overlay,

    // Node list (left panel)
    pub nodes: Vec<CachedNode>,
    pub selected_node_idx: usize,

    // Local images
    pub local_images: Vec<(String, u64)>,
    pub image_filter: String,

    // Cloud images (for download overlay)
    pub cloud_search: String,
    pub cloud_search_idx: usize,

    // Deploy wizard state
    pub deploy_image_idx: usize,
    pub deploy_disk_select_idx: usize,  // Which physical disk to use
    pub deploy_memory_idx: usize,
    pub deploy_cpu_idx: usize,
    pub deploy_disk_size_idx: usize,    // How much storage on that disk
    pub deploy_config_field: usize,     // 0=memory, 1=cpu, 2=disk size

    // Node init form
    pub node_init_hostname: String,
    pub node_init_mac: String,
    pub node_init_field: usize, // 0=hostname, 1=mac

    // Node action menu
    pub selected_action_idx: usize,

    // Status/feedback
    pub status_message: Option<(String, Instant)>,
    pub last_refresh: Instant,

    // Server status
    pub pxe_running: bool,
    pub http_port: u16,
}

impl App {
    pub fn new() -> Self {
        let nodes = load_node_cache();
        let local_images = Self::load_local_images();
        let (pxe_running, http_port) = Self::check_server_status();

        Self {
            running: true,
            overlay: Overlay::None,
            nodes,
            selected_node_idx: 0,
            local_images,
            image_filter: String::new(),
            cloud_search: String::new(),
            cloud_search_idx: 0,
            deploy_image_idx: 0,
            deploy_disk_select_idx: 0,
            deploy_memory_idx: 2, // Default 4GB
            deploy_cpu_idx: 1,    // Default 2 CPUs
            deploy_disk_size_idx: 2,   // Default 50GB
            deploy_config_field: 0,
            node_init_hostname: String::new(),
            node_init_mac: String::new(),
            node_init_field: 0,
            selected_action_idx: 0,
            status_message: None,
            last_refresh: Instant::now(),
            pxe_running,
            http_port,
        }
    }

    fn check_server_status() -> (bool, u16) {
        // Check if pixiecore is running
        // PID file format: "pixiecore_pid\nhttp_pid"
        let pid_file = Config::pixiecore_pid_file();
        let pxe_running = if pid_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                // First line is pixiecore PID
                if let Some(first_line) = content.lines().next() {
                    if let Ok(pid) = first_line.trim().parse::<i32>() {
                        // Check if process exists
                        std::path::Path::new(&format!("/proc/{}", pid)).exists()
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // Get HTTP port from config
        let http_port = Config::load()
            .map(|c| c.server.port)
            .unwrap_or(8080);

        (pxe_running, http_port)
    }

    fn load_local_images() -> Vec<(String, u64)> {
        let images_dir = Config::images_dir();
        let mut images = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&images_dir) {
            for entry in entries.flatten() {
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

                    if let Ok(meta) = std::fs::metadata(&path) {
                        images.push((name, meta.len()));
                    }
                }
            }
        }
        images.sort_by(|a, b| a.0.cmp(&b.0));
        images
    }

    pub fn refresh_data(&mut self) {
        self.nodes = load_node_cache();
        self.local_images = Self::load_local_images();
        let (pxe_running, http_port) = Self::check_server_status();
        self.pxe_running = pxe_running;
        self.http_port = http_port;
        self.last_refresh = Instant::now();

        // Clamp selection
        if !self.nodes.is_empty() && self.selected_node_idx >= self.nodes.len() {
            self.selected_node_idx = self.nodes.len() - 1;
        }
    }

    pub fn selected_node(&self) -> Option<&CachedNode> {
        self.nodes.get(self.selected_node_idx)
    }

    pub fn filtered_images(&self) -> Vec<&(String, u64)> {
        if self.image_filter.is_empty() {
            self.local_images.iter().collect()
        } else {
            let filter_lower = self.image_filter.to_lowercase();
            self.local_images
                .iter()
                .filter(|(name, _)| name.to_lowercase().contains(&filter_lower))
                .collect()
        }
    }

    pub fn filtered_cloud_images(&self) -> Vec<&'static CloudImage> {
        if self.cloud_search.is_empty() {
            CLOUD_IMAGES.iter().collect()
        } else {
            let search_lower = self.cloud_search.to_lowercase();
            CLOUD_IMAGES
                .iter()
                .filter(|img| {
                    img.name.to_lowercase().contains(&search_lower)
                        || img.version.to_lowercase().contains(&search_lower)
                })
                .collect()
        }
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), Instant::now()));
    }

    // Get max memory index based on node's RAM
    pub fn max_memory_idx(&self) -> usize {
        let Some(node) = self.selected_node() else {
            return MEMORY_OPTIONS.len() - 1;
        };

        // Parse RAM string (e.g., "7.5G", "16GB", "8192MB", "8192M")
        let ram_str = node.ram.to_uppercase();
        let ram_mb: u64 = if ram_str.contains("GB") {
            ram_str.replace("GB", "").trim().parse::<f64>().unwrap_or(0.0) as u64 * 1024
        } else if ram_str.ends_with("G") {
            ram_str.trim_end_matches('G').trim().parse::<f64>().unwrap_or(0.0) as u64 * 1024
        } else if ram_str.contains("MB") {
            ram_str.replace("MB", "").trim().parse::<u64>().unwrap_or(0)
        } else if ram_str.ends_with("M") {
            ram_str.trim_end_matches('M').trim().parse::<u64>().unwrap_or(0)
        } else {
            ram_str.trim().parse::<u64>().unwrap_or(0)
        };

        // Find highest option that fits
        MEMORY_OPTIONS.iter()
            .rposition(|(mb, _)| *mb <= ram_mb)
            .unwrap_or(0)
    }

    // Get max CPU index based on node's cores
    pub fn max_cpu_idx(&self) -> usize {
        let Some(node) = self.selected_node() else {
            return CPU_OPTIONS.len() - 1;
        };

        // Parse cores string (e.g., "4 cores", "4")
        let cores_str = node.cores.to_lowercase();
        let cores: u32 = cores_str
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        CPU_OPTIONS.iter()
            .rposition(|(c, _)| *c <= cores)
            .unwrap_or(0)
    }

    // Get max disk size index based on selected disk
    pub fn max_disk_size_idx(&self) -> usize {
        let Some(node) = self.selected_node() else {
            return DISK_OPTIONS.len() - 1;
        };

        let disk = node.disks.get(self.deploy_disk_select_idx);
        let disk_gb = disk.map(|d| d.size_bytes / (1024 * 1024 * 1024)).unwrap_or(0);

        DISK_OPTIONS.iter()
            .rposition(|(gb, _)| *gb <= disk_gb)
            .unwrap_or(0)
    }

    // Get selected disk info
    pub fn selected_disk_info(&self) -> Option<(usize, u64, &str)> {
        let node = self.selected_node()?;
        let disk = node.disks.get(self.deploy_disk_select_idx)?;
        let gb = disk.size_bytes / (1024 * 1024 * 1024);
        Some((self.deploy_disk_select_idx, gb, &disk.disk_type))
    }

    // ========================================================================
    // Key Handling
    // ========================================================================

    pub fn handle_key(&mut self, code: KeyCode) {
        match &self.overlay {
            Overlay::None => self.handle_main_keys(code),
            Overlay::NodeActions => self.handle_node_actions_keys(code),
            Overlay::Deploy(step) => {
                let step = step.clone();
                self.handle_deploy_keys(code, step);
            }
            Overlay::ImageDownload => self.handle_image_download_keys(code),
            Overlay::NodeInit => self.handle_node_init_keys(code),
            Overlay::Help => self.handle_help_keys(code),
        }
    }

    fn handle_main_keys(&mut self, code: KeyCode) {
        match code {
            // Quit
            KeyCode::Char('q') => self.running = false,

            // Help
            KeyCode::Char('?') => self.overlay = Overlay::Help,

            // Navigation
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.nodes.is_empty() {
                    self.selected_node_idx = (self.selected_node_idx + 1) % self.nodes.len();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !self.nodes.is_empty() {
                    self.selected_node_idx = self.selected_node_idx
                        .checked_sub(1)
                        .unwrap_or(self.nodes.len() - 1);
                }
            }

            // Open action menu for selected node
            KeyCode::Enter => {
                if self.selected_node().is_some() {
                    self.selected_action_idx = 0;
                    self.overlay = Overlay::NodeActions;
                }
            }

            // New node (doesn't require selection)
            KeyCode::Char('n') => {
                self.node_init_hostname.clear();
                self.node_init_mac.clear();
                self.node_init_field = 0;
                self.overlay = Overlay::NodeInit;
            }

            // Image download (doesn't require selection)
            KeyCode::Char('i') => {
                self.cloud_search.clear();
                self.cloud_search_idx = 0;
                self.overlay = Overlay::ImageDownload;
            }

            // Refresh / poll
            KeyCode::Char('r') => {
                let _ = Command::new("cave").args(["poll"]).output();
                self.refresh_data();
                self.set_status("Refreshed");
            }

            _ => {}
        }
    }

    fn handle_node_actions_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_action_idx = self.selected_action_idx
                    .checked_sub(1)
                    .unwrap_or(NODE_ACTIONS.len() - 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_action_idx = (self.selected_action_idx + 1) % NODE_ACTIONS.len();
            }
            KeyCode::Enter => {
                self.execute_selected_action();
            }
            _ => {}
        }
    }

    fn execute_selected_action(&mut self) {
        match self.selected_action_idx {
            0 => {
                // Deploy VM
                self.overlay = Overlay::None;
                self.start_deploy();
            }
            1 => {
                // Destroy VM
                self.execute_node_action("destroy");
                self.overlay = Overlay::None;
            }
            2 => {
                // Wake (WoL)
                self.execute_node_action("wake");
                self.overlay = Overlay::None;
            }
            3 => {
                // Shutdown
                self.execute_node_action("shutdown");
                self.overlay = Overlay::None;
            }
            4 => {
                // Restart
                self.execute_node_action("restart");
                self.overlay = Overlay::None;
            }
            5 => {
                // Remove Node
                self.execute_node_action("remove");
                self.overlay = Overlay::None;
            }
            _ => {}
        }
    }

    fn handle_deploy_keys(&mut self, code: KeyCode, step: DeployStep) {
        match step {
            DeployStep::SelectImage => match code {
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    let images = self.filtered_images();
                    if !images.is_empty() {
                        self.deploy_image_idx = self.deploy_image_idx
                            .checked_sub(1)
                            .unwrap_or(images.len() - 1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let images = self.filtered_images();
                    if !images.is_empty() {
                        self.deploy_image_idx = (self.deploy_image_idx + 1) % images.len();
                    }
                }
                KeyCode::Enter => {
                    if !self.filtered_images().is_empty() {
                        self.deploy_disk_select_idx = 0;
                        self.overlay = Overlay::Deploy(DeployStep::SelectDisk);
                    }
                }
                KeyCode::Backspace => {
                    self.image_filter.pop();
                    self.deploy_image_idx = 0;
                }
                KeyCode::Char(c) => {
                    self.image_filter.push(c);
                    self.deploy_image_idx = 0;
                }
                _ => {}
            },

            DeployStep::SelectDisk => match code {
                KeyCode::Esc => self.overlay = Overlay::Deploy(DeployStep::SelectImage),
                KeyCode::Up | KeyCode::Char('k') => {
                    self.deploy_disk_select_idx = self.deploy_disk_select_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(node) = self.selected_node() {
                        if self.deploy_disk_select_idx < node.disks.len().saturating_sub(1) {
                            self.deploy_disk_select_idx += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    // Clamp disk size to selected disk's capacity
                    let max_disk = self.max_disk_size_idx();
                    self.deploy_disk_size_idx = self.deploy_disk_size_idx.min(max_disk);
                    self.overlay = Overlay::Deploy(DeployStep::Configure);
                }
                _ => {}
            },

            DeployStep::Configure => match code {
                KeyCode::Esc => self.overlay = Overlay::Deploy(DeployStep::SelectDisk),
                KeyCode::Up | KeyCode::Char('k') => {
                    self.deploy_config_field = self.deploy_config_field.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.deploy_config_field < 2 {
                        self.deploy_config_field += 1;
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    match self.deploy_config_field {
                        0 => self.deploy_memory_idx = self.deploy_memory_idx.saturating_sub(1),
                        1 => self.deploy_cpu_idx = self.deploy_cpu_idx.saturating_sub(1),
                        2 => self.deploy_disk_size_idx = self.deploy_disk_size_idx.saturating_sub(1),
                        _ => {}
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let max_mem = self.max_memory_idx();
                    let max_cpu = self.max_cpu_idx();
                    let max_disk = self.max_disk_size_idx();
                    match self.deploy_config_field {
                        0 if self.deploy_memory_idx < max_mem => {
                            self.deploy_memory_idx += 1;
                        }
                        1 if self.deploy_cpu_idx < max_cpu => {
                            self.deploy_cpu_idx += 1;
                        }
                        2 if self.deploy_disk_size_idx < max_disk => {
                            self.deploy_disk_size_idx += 1;
                        }
                        _ => {}
                    }
                }
                KeyCode::Enter => {
                    self.overlay = Overlay::Deploy(DeployStep::Confirm);
                }
                _ => {}
            },

            DeployStep::Confirm => match code {
                KeyCode::Esc => self.overlay = Overlay::Deploy(DeployStep::Configure),
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.overlay = Overlay::Deploy(DeployStep::Deploying);
                    self.execute_deploy();
                }
                KeyCode::Char('n') => self.overlay = Overlay::None,
                _ => {}
            },

            DeployStep::Deploying => {
                // No input during deploy
            }

            DeployStep::Done => match code {
                KeyCode::Enter | KeyCode::Esc => {
                    self.overlay = Overlay::None;
                    self.refresh_data();
                }
                _ => {}
            },
        }
    }

    fn handle_image_download_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up => {
                let images = self.filtered_cloud_images();
                if !images.is_empty() {
                    self.cloud_search_idx = self.cloud_search_idx
                        .checked_sub(1)
                        .unwrap_or(images.len() - 1);
                }
            }
            KeyCode::Down => {
                let images = self.filtered_cloud_images();
                if !images.is_empty() {
                    self.cloud_search_idx = (self.cloud_search_idx + 1) % images.len();
                }
            }
            KeyCode::Enter => {
                let images = self.filtered_cloud_images();
                if let Some(img) = images.get(self.cloud_search_idx) {
                    self.download_image(img.url);
                    self.overlay = Overlay::None;
                }
            }
            KeyCode::Backspace => {
                self.cloud_search.pop();
                self.cloud_search_idx = 0;
            }
            KeyCode::Char(c) => {
                self.cloud_search.push(c);
                self.cloud_search_idx = 0;
            }
            _ => {}
        }
    }

    fn handle_node_init_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Tab | KeyCode::Down => {
                self.node_init_field = (self.node_init_field + 1) % 2;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.node_init_field = if self.node_init_field == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                if !self.node_init_hostname.is_empty() && !self.node_init_mac.is_empty() {
                    self.execute_node_init();
                    self.overlay = Overlay::None;
                }
            }
            KeyCode::Backspace => {
                match self.node_init_field {
                    0 => { self.node_init_hostname.pop(); }
                    1 => { self.node_init_mac.pop(); }
                    _ => {}
                }
            }
            KeyCode::Char(c) => {
                match self.node_init_field {
                    0 => self.node_init_hostname.push(c),
                    1 => {
                        // Auto-format MAC address
                        let clean: String = self.node_init_mac
                            .chars()
                            .filter(|c| c.is_ascii_hexdigit())
                            .collect();
                        if clean.len() < 12 && c.is_ascii_hexdigit() {
                            self.node_init_mac.push(c.to_ascii_uppercase());
                            // Add colons
                            let new_clean: String = self.node_init_mac
                                .chars()
                                .filter(|c| c.is_ascii_hexdigit())
                                .collect();
                            if new_clean.len() % 2 == 0 && new_clean.len() < 12 {
                                self.node_init_mac.push(':');
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_help_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.overlay = Overlay::None;
            }
            _ => {}
        }
    }

    // ========================================================================
    // Actions
    // ========================================================================

    fn start_deploy(&mut self) {
        self.deploy_image_idx = 0;
        self.image_filter.clear();
        self.deploy_config_field = 0;

        // Set defaults clamped to node's capacity
        let max_mem = self.max_memory_idx();
        let max_cpu = self.max_cpu_idx();

        // Default to 4GB/2CPU or max available, whichever is smaller
        // Disk size will be clamped after disk selection
        self.deploy_memory_idx = 2.min(max_mem);  // 4GB default
        self.deploy_cpu_idx = 1.min(max_cpu);     // 2 CPUs default
        self.deploy_disk_select_idx = 0;          // First disk
        self.deploy_disk_size_idx = 2;            // 50GB default (will be clamped)

        self.overlay = Overlay::Deploy(DeployStep::SelectImage);
    }

    fn execute_node_action(&mut self, action: &str) {
        if let Some(node) = self.selected_node() {
            let hostname = node.hostname.clone();
            let result = Command::new("cave")
                .args(["node", action, &hostname])
                .output();

            match result {
                Ok(output) => {
                    if output.status.success() {
                        self.set_status(&format!("{} {}: success", action, hostname));
                    } else {
                        let err = String::from_utf8_lossy(&output.stderr);
                        self.set_status(&format!("{} failed: {}", action, err.trim()));
                    }
                }
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
            self.refresh_data();
        }
    }

    fn execute_deploy(&mut self) {
        let images = self.filtered_images();
        let image_name = match images.get(self.deploy_image_idx) {
            Some((name, _)) => name.clone(),
            None => {
                self.overlay = Overlay::Deploy(DeployStep::Done);
                return;
            }
        };

        let hostname = match self.selected_node() {
            Some(node) => node.hostname.clone(),
            None => {
                self.overlay = Overlay::Deploy(DeployStep::Done);
                return;
            }
        };

        let memory = MEMORY_OPTIONS[self.deploy_memory_idx].0;
        let cpus = CPU_OPTIONS[self.deploy_cpu_idx].0;
        let disk = DISK_OPTIONS[self.deploy_disk_size_idx].0;

        let result = Command::new("cave")
            .args([
                "node", "deploy", &hostname,
                "--image", &image_name,
                "--memory", &memory.to_string(),
                "--cpus", &cpus.to_string(),
                "--disk", &disk.to_string(),
            ])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    self.set_status(&format!("Deployed to {}", hostname));
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    self.set_status(&format!("Deploy failed: {}", err.trim()));
                }
            }
            Err(e) => self.set_status(&format!("Deploy error: {}", e)),
        }

        self.overlay = Overlay::Deploy(DeployStep::Done);
    }

    fn execute_node_init(&mut self) {
        let result = Command::new("cave")
            .args([
                "node", "init",
                &self.node_init_hostname,
                &self.node_init_mac,
            ])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    self.set_status(&format!("Added node: {}", self.node_init_hostname));
                    // Run poll to update cache with new node
                    let _ = Command::new("cave").args(["poll"]).output();
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    self.set_status(&format!("Failed: {}", err.trim()));
                }
            }
            Err(e) => self.set_status(&format!("Error: {}", e)),
        }

        self.refresh_data();
    }

    fn download_image(&mut self, url: &str) {
        self.set_status(&format!("Downloading... (see terminal)"));

        // Run in background - user will see progress in terminal
        let _ = Command::new("cave")
            .args(["image", "pull", url])
            .spawn();
    }

}

// ============================================================================
// Main Run Loop
// ============================================================================

pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    // Auto-refresh (poll) every 10 seconds
    let refresh_interval = Duration::from_secs(10);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();

            // Clear old status messages
            if let Some((_, created)) = &app.status_message {
                if created.elapsed() > Duration::from_secs(5) {
                    app.status_message = None;
                }
            }
        }

        // Auto-refresh (run poll in background)
        if app.last_refresh.elapsed() >= refresh_interval {
            // Run poll in background to update cache
            let _ = Command::new("cave").args(["poll"]).spawn();
            app.refresh_data();
        }

        if !app.running {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
