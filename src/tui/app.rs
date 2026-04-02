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
use crate::config::{load_discovered_cache, load_node_cache, save_node_cache, CachedNode, Config};

use super::ui;

// ============================================================================
// Overlay Screens (popups over the main view)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NodeActions,  // Action menu for selected node
    Deploy(DeployStep),
    ActionProgress(String),  // Running an action (e.g., "Destroying VM...")
    ConfirmRemove,  // Confirm before removing a node (wipes VM data)
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
    (49152, "48 GB"),
    (65536, "64 GB"),
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
    (128, "128 GB"),
    (200, "200 GB"),
    (250, "250 GB"),
    (500, "500 GB"),
    (512, "512 GB"),
    (1000, "1 TB"),
    (2000, "2 TB"),
    (4000, "4 TB"),
];

pub const NODE_ACTIONS: &[&str] = &[
    "Deploy VM",
    "Destroy VM",
    "Wake (WoL)",
    "Shutdown",
    "Restart",
    "Remove Node",
];

pub const DISCOVERED_NODE_ACTIONS: &[&str] = &[
    "Initialize",
    "Destroy VM",
    "Wake (WoL)",
    "Shutdown",
    "Restart",
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
    pub deploy_pending: bool,           // True when deploy should run on next tick
    pub deploy_waiting_for_ip: bool,    // True when waiting for VM to get IP
    pub deploy_target: Option<String>,  // Hostname we're deploying to
    pub deploy_wait_start: Option<Instant>, // When we started waiting
    pub deploy_last_poll: Option<Instant>,  // Last time we polled for VM IP
    pub destroy_waiting: bool,              // True when waiting for VM to be destroyed
    pub destroy_target: Option<String>,     // Hostname we're waiting to confirm destroy
    pub destroy_wait_start: Option<Instant>,
    pub destroy_last_poll: Option<Instant>,
    pub pending_action: Option<String>, // Pending node action (destroy, wake, etc.)
    pub deploy_handle: Option<std::process::Child>, // Running deploy command

    // Discovered (uninitialized) nodes from PXE boot (hostname is empty)
    pub discovered_nodes: Vec<CachedNode>,

    // Node init form
    pub node_init_hostname: String,
    pub node_init_mac: String,
    pub node_init_field: usize, // 0=hostname, 1=mac
    pub node_init_pending: bool, // True when adding node (show loading)

    // Node action menu
    pub selected_action_idx: usize,

    // Status/feedback
    pub status_message: Option<(String, Instant)>,
    pub last_refresh: Instant,
    pub nodes_frozen_until: Option<Instant>,  // Don't refresh nodes until this time

    // Server status
    pub pxe_running: bool,
    pub http_port: u16,

    // Continuous polling
    pub poll_handle: Option<std::process::Child>,

    // Image download tracking
    pub image_download_handle: Option<std::process::Child>,
    pub image_download_name: Option<String>,
    pub image_download_start: Option<Instant>,
    pub image_download_total_bytes: Option<u64>,
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
            deploy_pending: false,
            deploy_waiting_for_ip: false,
            deploy_target: None,
            deploy_wait_start: None,
            deploy_last_poll: None,
            discovered_nodes: load_discovered_cache(),
            deploy_handle: None,
            destroy_waiting: false,
            destroy_target: None,
            destroy_wait_start: None,
            destroy_last_poll: None,
            pending_action: None,
            node_init_hostname: String::new(),
            node_init_mac: String::new(),
            node_init_field: 0,
            node_init_pending: false,
            selected_action_idx: 0,
            status_message: None,
            last_refresh: Instant::now(),
            nodes_frozen_until: None,
            pxe_running,
            http_port,
            poll_handle: None,
            image_download_handle: None,
            image_download_name: None,
            image_download_start: None,
            image_download_total_bytes: None,
        }
    }

    /// Start a poll if one isn't already running
    pub fn start_poll_if_idle(&mut self) {
        // Check if previous poll finished
        if let Some(ref mut handle) = self.poll_handle {
            match handle.try_wait() {
                Ok(Some(_)) => {
                    // Poll finished, clear handle
                    self.poll_handle = None;
                }
                Ok(None) => {
                    // Still running, don't start another
                    return;
                }
                Err(_) => {
                    // Error checking, clear and restart
                    self.poll_handle = None;
                }
            }
        }

        // Start new poll
        if let Ok(child) = Command::new("cave").args(["poll"]).spawn() {
            self.poll_handle = Some(child);
        }
    }

    /// Check if poll completed and refresh data
    pub fn check_poll_complete(&mut self) -> bool {
        if let Some(ref mut handle) = self.poll_handle {
            match handle.try_wait() {
                Ok(Some(_)) => {
                    // Poll finished, refresh data
                    self.poll_handle = None;
                    self.refresh_data();
                    self.last_refresh = Instant::now();
                    return true;
                }
                _ => {}
            }
        }
        false
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
        // Skip refreshing nodes if frozen (recently added/removed a node)
        let nodes_frozen = self.nodes_frozen_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false);

        if !nodes_frozen {
            self.nodes = load_node_cache();
            self.discovered_nodes = load_discovered_cache();
        }

        self.local_images = Self::load_local_images();
        let (pxe_running, http_port) = Self::check_server_status();
        self.pxe_running = pxe_running;
        self.http_port = http_port;
        self.last_refresh = Instant::now();

        // Clamp selection
        let total = self.nodes.len() + self.discovered_nodes.len();
        if total > 0 && self.selected_node_idx >= total {
            self.selected_node_idx = total - 1;
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

    /// Check if VM has gotten an IP yet. Returns Some(ip) if found.
    pub fn check_vm_ip(&self, hostname: &str) -> Option<String> {
        self.nodes.iter()
            .find(|n| n.hostname == hostname)
            .and_then(|n| n.vm.as_ref())
            .filter(|vm| !vm.ip.is_empty())
            .map(|vm| vm.ip.clone())
    }

    /// Poll and check if waiting VM has IP. Returns true if done waiting.
    pub fn check_deploy_complete(&mut self) -> bool {
        if !self.deploy_waiting_for_ip {
            return false;
        }

        // Only poll every 3 seconds
        let should_poll = match self.deploy_last_poll {
            Some(last) => last.elapsed() >= Duration::from_secs(3),
            None => true,
        };

        if !should_poll {
            return false;
        }

        self.deploy_last_poll = Some(Instant::now());

        let hostname = match &self.deploy_target {
            Some(h) => h.clone(),
            None => {
                self.deploy_waiting_for_ip = false;
                return true;
            }
        };

        // Poll to refresh cache
        let _ = Command::new("cave").args(["poll"]).output();
        self.refresh_data();

        // Check if VM has IP
        if let Some(ip) = self.check_vm_ip(&hostname) {
            self.deploy_waiting_for_ip = false;
            self.deploy_target = None;
            self.deploy_wait_start = None;
            self.deploy_last_poll = None;
            self.overlay = Overlay::None;
            self.set_status(&format!("VM running at {}", ip));
            return true;
        }

        // Timeout after 2 minutes
        if let Some(start) = self.deploy_wait_start {
            if start.elapsed() > Duration::from_secs(120) {
                self.deploy_waiting_for_ip = false;
                self.deploy_target = None;
                self.deploy_wait_start = None;
                self.deploy_last_poll = None;
                self.overlay = Overlay::None;
                self.set_status("VM deployed but IP not found (timeout)");
                return true;
            }
        }

        false
    }

    /// Poll and check if VM has been destroyed. Returns true if done waiting.
    pub fn check_destroy_complete(&mut self) -> bool {
        if !self.destroy_waiting {
            return false;
        }

        // Only poll every 3 seconds
        let should_poll = match self.destroy_last_poll {
            Some(last) => last.elapsed() >= Duration::from_secs(3),
            None => true,
        };

        if !should_poll {
            return false;
        }

        self.destroy_last_poll = Some(Instant::now());

        let hostname = match &self.destroy_target {
            Some(h) => h.clone(),
            None => {
                self.destroy_waiting = false;
                return true;
            }
        };

        // Poll to refresh cache
        let _ = Command::new("cave").args(["poll"]).output();
        self.refresh_data();

        // Check if VM is gone (node has no VM or VM is None)
        let vm_gone = self.nodes.iter()
            .find(|n| n.hostname == hostname)
            .map(|n| n.vm.is_none())
            .unwrap_or(true);

        if vm_gone {
            self.destroy_waiting = false;
            self.destroy_target = None;
            self.destroy_wait_start = None;
            self.destroy_last_poll = None;
            self.overlay = Overlay::None;
            self.set_status(&format!("VM destroyed on {}", hostname));
            return true;
        }

        // Timeout after 30 seconds
        if let Some(start) = self.destroy_wait_start {
            if start.elapsed() > Duration::from_secs(30) {
                self.destroy_waiting = false;
                self.destroy_target = None;
                self.destroy_wait_start = None;
                self.destroy_last_poll = None;
                self.overlay = Overlay::None;
                self.set_status("Destroy completed (timeout waiting for confirmation)");
                return true;
            }
        }

        false
    }

    /// Check if image download completed. Returns true if done.
    pub fn check_download_complete(&mut self) -> bool {
        if let Some(ref mut handle) = self.image_download_handle {
            match handle.try_wait() {
                Ok(Some(status)) => {
                    let name = self.image_download_name.take().unwrap_or_default();
                    self.image_download_handle = None;
                    self.image_download_start = None;
                    self.image_download_total_bytes = None;
                    self.overlay = Overlay::None;

                    if status.success() {
                        self.local_images = Self::load_local_images();
                        self.set_status(&format!("Downloaded: {}", name));
                    } else {
                        self.set_status(&format!("Download failed: {}", name));
                    }
                    return true;
                }
                Ok(None) => {
                    // Still running
                    return false;
                }
                Err(_) => {
                    self.image_download_handle = None;
                    self.image_download_name = None;
                    self.image_download_start = None;
                    self.image_download_total_bytes = None;
                    self.overlay = Overlay::None;
                    self.set_status("Download error");
                    return true;
                }
            }
        }
        false
    }

    /// Get download elapsed time as formatted string
    pub fn download_elapsed(&self) -> String {
        if let Some(start) = self.image_download_start {
            let secs = start.elapsed().as_secs();
            format!("{}:{:02}", secs / 60, secs % 60)
        } else {
            "0:00".to_string()
        }
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

    // Get disk size options for the selected disk (includes max capacity)
    pub fn get_disk_options(&self) -> Vec<(u64, String)> {
        let max_gb = self.selected_node()
            .and_then(|n| n.disks.get(self.deploy_disk_select_idx))
            .map(|d| d.size_bytes / 1_000_000_000)
            .unwrap_or(500);

        let mut options: Vec<(u64, String)> = DISK_OPTIONS
            .iter()
            .filter(|(gb, _)| *gb <= max_gb)
            .map(|(gb, label)| (*gb, label.to_string()))
            .collect();

        // Add max capacity if not already in list
        if !options.iter().any(|(gb, _)| *gb == max_gb) && max_gb > 0 {
            options.push((max_gb, format!("{} GB (max)", max_gb)));
        }

        options
    }

    // Get max disk size index based on selected disk
    pub fn max_disk_size_idx(&self) -> usize {
        self.get_disk_options().len().saturating_sub(1)
    }

    // Get the actual disk size value at current index
    pub fn selected_disk_size_gb(&self) -> u64 {
        let options = self.get_disk_options();
        options.get(self.deploy_disk_size_idx)
            .map(|(gb, _)| *gb)
            .unwrap_or(50)
    }

    // Get disk size label at current index
    pub fn selected_disk_size_label(&self) -> String {
        let options = self.get_disk_options();
        options.get(self.deploy_disk_size_idx)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "50 GB".to_string())
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
            Overlay::ConfirmRemove => {
                match code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        self.pending_action = Some("remove".to_string());
                        self.overlay = Overlay::ActionProgress("Removing node and wiping VM data...".to_string());
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.overlay = Overlay::None;
                    }
                    _ => {}
                }
            }
            Overlay::ActionProgress(_) => {} // No input during action
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
                let total = self.nodes.len() + self.discovered_nodes.len();
                if total > 0 {
                    self.selected_node_idx = (self.selected_node_idx + 1) % total;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let total = self.nodes.len() + self.discovered_nodes.len();
                if total > 0 {
                    self.selected_node_idx = self.selected_node_idx
                        .checked_sub(1)
                        .unwrap_or(total - 1);
                }
            }

            // Open action menu for selected node
            KeyCode::Enter => {
                let total = self.nodes.len() + self.discovered_nodes.len();
                if self.selected_node_idx < total {
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

    /// Whether the currently selected node is a discovered (uninitialized) node
    pub fn is_selected_discovered(&self) -> bool {
        self.selected_node_idx >= self.nodes.len()
    }

    /// Get the currently selected discovered node
    pub fn selected_discovered(&self) -> Option<&CachedNode> {
        if self.is_selected_discovered() {
            self.discovered_nodes.get(self.selected_node_idx - self.nodes.len())
        } else {
            None
        }
    }

    fn handle_node_actions_keys(&mut self, code: KeyCode) {
        let actions = if self.is_selected_discovered() {
            DISCOVERED_NODE_ACTIONS
        } else {
            NODE_ACTIONS
        };
        match code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_action_idx = self.selected_action_idx
                    .checked_sub(1)
                    .unwrap_or(actions.len() - 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_action_idx = (self.selected_action_idx + 1) % actions.len();
            }
            KeyCode::Enter => {
                self.execute_selected_action();
            }
            _ => {}
        }
    }

    fn execute_selected_action(&mut self) {
        if self.is_selected_discovered() {
            // Discovered node actions: Initialize, Destroy, Wake, Shutdown, Restart
            match self.selected_action_idx {
                0 => {
                    // Initialize - open init form with MAC pre-filled
                    if let Some(disc) = self.selected_discovered().cloned() {
                        self.node_init_hostname.clear();
                        self.node_init_mac = disc.mac.clone();
                        self.node_init_field = 0;
                        self.overlay = Overlay::NodeInit;
                    }
                }
                1 => {
                    self.pending_action = Some("destroy".to_string());
                    self.overlay = Overlay::ActionProgress("Destroying VM...".to_string());
                }
                2 => {
                    self.pending_action = Some("wake".to_string());
                    self.overlay = Overlay::ActionProgress("Waking node...".to_string());
                }
                3 => {
                    self.pending_action = Some("shutdown".to_string());
                    self.overlay = Overlay::ActionProgress("Shutting down...".to_string());
                }
                4 => {
                    self.pending_action = Some("restart".to_string());
                    self.overlay = Overlay::ActionProgress("Restarting...".to_string());
                }
                _ => {}
            }
        } else {
            // Registered node actions
            match self.selected_action_idx {
                0 => {
                    // Deploy VM
                    self.overlay = Overlay::None;
                    self.start_deploy();
                }
                1 => {
                    // Destroy VM
                    self.pending_action = Some("destroy".to_string());
                    self.overlay = Overlay::ActionProgress("Destroying VM...".to_string());
                }
                2 => {
                    // Wake (WoL)
                    self.pending_action = Some("wake".to_string());
                    self.overlay = Overlay::ActionProgress("Waking node...".to_string());
                }
                3 => {
                    // Shutdown
                    self.pending_action = Some("shutdown".to_string());
                    self.overlay = Overlay::ActionProgress("Shutting down...".to_string());
                }
                4 => {
                    // Restart
                    self.pending_action = Some("restart".to_string());
                    self.overlay = Overlay::ActionProgress("Restarting...".to_string());
                }
                5 => {
                    // Remove Node - show confirmation first
                    self.overlay = Overlay::ConfirmRemove;
                }
                _ => {}
            }
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
                    // Set pending flag - deploy will run on next tick after UI updates
                    self.deploy_pending = true;
                    self.overlay = Overlay::Deploy(DeployStep::Deploying);
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
                    // Show loading overlay, execute on next tick
                    self.node_init_pending = true;
                    self.overlay = Overlay::ActionProgress("Adding node...".to_string());
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
        if self.is_selected_discovered() {
            self.execute_discovered_node_action(action);
            return;
        }

        if let Some(node) = self.selected_node() {
            let hostname = node.hostname.clone();

            // Build args - destroy and remove need -y to skip confirmation
            let args: Vec<&str> = if action == "destroy" || action == "remove" {
                vec!["node", action, &hostname, "-y"]
            } else {
                vec!["node", action, &hostname]
            };

            let result = Command::new("cave")
                .args(&args)
                .output();

            match result {
                Ok(output) => {
                    if output.status.success() {
                        // For destroy, enter waiting state to confirm VM is gone
                        if action == "destroy" {
                            self.destroy_waiting = true;
                            self.destroy_target = Some(hostname.clone());
                            self.destroy_wait_start = Some(Instant::now());
                            self.overlay = Overlay::ActionProgress("Confirming VM destroyed...".to_string());
                            return; // Don't clear overlay yet
                        }

                        // For remove, immediately remove from list and save cache
                        if action == "remove" {
                            // Kill any running poll and WAIT for it to die
                            if let Some(mut handle) = self.poll_handle.take() {
                                let _ = handle.kill();
                                let _ = handle.wait(); // Wait for process to actually terminate
                            }

                            self.nodes.retain(|n| n.hostname != hostname);
                            let _ = save_node_cache(&self.nodes);
                            // Freeze node list for 5 seconds to prevent stale poll data from overwriting
                            self.nodes_frozen_until = Some(Instant::now() + Duration::from_secs(5));
                            // Adjust selection if needed
                            if self.selected_node_idx >= self.nodes.len() && !self.nodes.is_empty() {
                                self.selected_node_idx = self.nodes.len() - 1;
                            }
                        }

                        self.set_status(&format!("{} {}: success", action, hostname));
                    } else {
                        let err = String::from_utf8_lossy(&output.stderr);
                        self.set_status(&format!("{} failed: {}", action, err.trim()));
                    }
                }
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    /// Execute actions on discovered nodes directly (not via cave CLI)
    fn execute_discovered_node_action(&mut self, action: &str) {
        let Some(disc) = self.selected_discovered().cloned() else { return };
        let label = disc.ip.as_deref().unwrap_or(&disc.mac);

        match action {
            "wake" => {
                // Send WoL magic packet directly
                if let Ok(mac_bytes) = Self::parse_mac(&disc.mac) {
                    let mut packet = vec![0xFFu8; 6];
                    for _ in 0..16 {
                        packet.extend_from_slice(&mac_bytes);
                    }
                    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                        let _ = socket.set_broadcast(true);
                        if socket.send_to(&packet, "255.255.255.255:9").is_ok() {
                            self.set_status(&format!("WoL sent to {}", label));
                        } else {
                            self.set_status("Failed to send WoL packet");
                        }
                    }
                } else {
                    self.set_status("Invalid MAC address");
                }
            }
            "shutdown" | "restart" => {
                if let Some(ref ip) = disc.ip {
                    let cmd = if action == "shutdown" { "poweroff" } else { "reboot" };
                    let result = Command::new("ssh")
                        .args([
                            "-i", &Config::ssh_private_key().to_string_lossy(),
                            "-o", "StrictHostKeyChecking=no",
                            "-o", "ConnectTimeout=5",
                            &format!("root@{}", ip),
                            &format!("nohup {} &", cmd),
                        ])
                        .output();
                    match result {
                        Ok(_) => self.set_status(&format!("{} sent to {}", action, label)),
                        Err(e) => self.set_status(&format!("{} failed: {}", action, e)),
                    }
                } else {
                    self.set_status("No IP address available");
                }
            }
            "destroy" => {
                if let Some(ref ip) = disc.ip {
                    // Kill any running VMs on the node
                    let result = Command::new("ssh")
                        .args([
                            "-i", &Config::ssh_private_key().to_string_lossy(),
                            "-o", "StrictHostKeyChecking=no",
                            "-o", "ConnectTimeout=5",
                            &format!("root@{}", ip),
                            "killall qemu-system-x86_64 2>/dev/null; rm -f /var/run/cave/*.pid",
                        ])
                        .output();
                    match result {
                        Ok(_) => self.set_status(&format!("VM destroyed on {}", label)),
                        Err(e) => self.set_status(&format!("destroy failed: {}", e)),
                    }
                } else {
                    self.set_status("No IP address available");
                }
            }
            _ => {}
        }
    }

    fn parse_mac(mac: &str) -> Result<[u8; 6], ()> {
        let bytes: Vec<u8> = mac
            .split(':')
            .filter_map(|s| u8::from_str_radix(s, 16).ok())
            .collect();
        if bytes.len() != 6 { return Err(()); }
        let mut arr = [0u8; 6];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }

    fn execute_deploy(&mut self) {
        let images = self.filtered_images();
        let image_name = match images.get(self.deploy_image_idx) {
            Some((name, _)) => name.clone(),
            None => {
                self.set_status("No image selected");
                self.overlay = Overlay::None;
                return;
            }
        };

        let hostname = match self.selected_node() {
            Some(node) => node.hostname.clone(),
            None => {
                self.set_status("No node selected");
                self.overlay = Overlay::None;
                return;
            }
        };

        let memory = MEMORY_OPTIONS[self.deploy_memory_idx].0;
        let cpus = CPU_OPTIONS[self.deploy_cpu_idx].0;
        let disk = self.selected_disk_size_gb();

        // Log output to file for debugging
        let log_path = Config::cave_dir().join("deploy.log");
        let log_file = std::fs::File::create(&log_path).ok();
        let stdout = log_file.as_ref()
            .and_then(|f| f.try_clone().ok())
            .map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null);
        let stderr = log_file
            .map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null);

        // Spawn deploy command in background (non-blocking)
        match Command::new("cave")
            .args([
                "node", "deploy", &hostname, &image_name,
                "--memory", &memory.to_string(),
                "--cpus", &cpus.to_string(),
                "--disk", &disk.to_string(),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
        {
            Ok(child) => {
                self.deploy_handle = Some(child);
                self.deploy_target = Some(hostname.clone());
                self.deploy_wait_start = Some(Instant::now());
                self.overlay = Overlay::ActionProgress(format!("Deploying to {}...", hostname));
            }
            Err(e) => {
                self.set_status(&format!("Deploy error: {}", e));
                self.overlay = Overlay::None;
            }
        }
    }

    fn execute_node_init(&mut self) {
        let hostname = self.node_init_hostname.clone();
        let mac = self.node_init_mac.clone();

        let result = Command::new("cave")
            .args([
                "node", "init",
                &hostname,
                &mac,
            ])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    // Kill any running poll and WAIT for it to die
                    if let Some(mut handle) = self.poll_handle.take() {
                        let _ = handle.kill();
                        let _ = handle.wait(); // Wait for process to actually terminate
                    }

                    // Add node to list immediately (no need to wait for poll)
                    self.nodes.push(CachedNode {
                        hostname: hostname.clone(),
                        mac,
                        ip: None,
                        status: "offline".to_string(),
                        cpu: String::new(),
                        cores: String::new(),
                        ram: String::new(),
                        ram_total_mb: None,
                        ram_used_mb: None,
                        disks: vec![],
                        vm: None,
                    });
                    // Save to cache file so refresh_data doesn't lose it
                    let _ = save_node_cache(&self.nodes);
                    // Freeze node list for 5 seconds to prevent stale poll data from overwriting
                    self.nodes_frozen_until = Some(Instant::now() + Duration::from_secs(5));
                    self.set_status(&format!("Added node: {}", hostname));
                    self.overlay = Overlay::None;
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    self.set_status(&format!("Failed: {}", err.trim()));
                    self.overlay = Overlay::None;
                }
            }
            Err(e) => {
                self.set_status(&format!("Error: {}", e));
                self.overlay = Overlay::None;
            }
        }
    }

    fn download_image(&mut self, url: &str) {
        // Extract image name from URL
        let name = url.split('/').last().unwrap_or("image").to_string();

        // Get total file size via curl HEAD request
        let total_bytes = Command::new("curl")
            .args(["-sI", url])
            .output()
            .ok()
            .and_then(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("content-length:"))
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|s| s.trim().parse::<u64>().ok())
            });

        // Log file for debugging
        let log_path = Config::cave_dir().join("download.log");
        let log_file = std::fs::File::create(&log_path).ok();
        let stdout = log_file.as_ref()
            .and_then(|f| f.try_clone().ok())
            .map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null);
        let stderr = log_file
            .map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null);

        // Run in background with output to log file
        match Command::new("cave")
            .args(["image", "pull", url])
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
        {
            Ok(child) => {
                self.image_download_handle = Some(child);
                self.image_download_name = Some(name.clone());
                self.image_download_start = Some(Instant::now());
                self.image_download_total_bytes = total_bytes;
                self.overlay = Overlay::ActionProgress(format!("Downloading {}...", name));
            }
            Err(e) => {
                self.set_status(&format!("Download failed: {}", e));
            }
        }
    }

    /// Get current download progress by checking file size
    pub fn download_progress(&self) -> String {
        if let Some(ref name) = self.image_download_name {
            let path = Config::images_dir().join(name);
            let current_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

            // Format current size
            let current_str = if current_bytes >= 1_000_000_000 {
                format!("{:.1} GB", current_bytes as f64 / 1_000_000_000.0)
            } else {
                format!("{:.0} MB", current_bytes as f64 / 1_000_000.0)
            };

            // If we know total size, show progress bar
            if let Some(total) = self.image_download_total_bytes {
                if total > 0 {
                    let percent = (current_bytes as f64 / total as f64 * 100.0).min(100.0);
                    let filled = (percent / 5.0) as usize; // 20 chars total
                    let empty = 20 - filled;
                    let bar = format!("{}{}",
                        "█".repeat(filled),
                        "░".repeat(empty)
                    );

                    let total_str = if total >= 1_000_000_000 {
                        format!("{:.1} GB", total as f64 / 1_000_000_000.0)
                    } else {
                        format!("{:.0} MB", total as f64 / 1_000_000.0)
                    };

                    return format!("[{}] {:.0}% ({}/{})", bar, percent, current_str, total_str);
                }
            }

            // Fallback: just show current size
            return current_str;
        }
        "0 MB".to_string()
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

    // Start initial poll immediately
    app.start_poll_if_idle();

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Check for pending deploy (runs after UI draws "Deploying...")
        if app.deploy_pending {
            app.deploy_pending = false;
            app.execute_deploy();
            continue; // Redraw immediately after deploy
        }

        // Check for pending node action (runs after UI draws progress)
        if let Some(action) = app.pending_action.take() {
            app.execute_node_action(&action);
            // Don't clear overlay if we're now in a waiting state
            if !app.destroy_waiting {
                app.overlay = Overlay::None;
            }
            continue; // Redraw immediately after action
        }

        // Check for pending node init (runs after UI draws progress)
        if app.node_init_pending {
            app.node_init_pending = false;
            app.execute_node_init();
            // Don't clear overlay - execute_node_init sets its own overlay for waiting
            continue; // Redraw immediately after init
        }

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

            // Check if deploy command finished
            if app.deploy_handle.is_some() {
                if let Some(ref mut handle) = app.deploy_handle {
                    match handle.try_wait() {
                        Ok(Some(status)) => {
                            app.deploy_handle = None;
                            if status.success() {
                                // Deploy done, now wait for IP
                                app.deploy_waiting_for_ip = true;
                                app.deploy_wait_start = Some(Instant::now());
                                let target = app.deploy_target.as_deref().unwrap_or("VM");
                                app.overlay = Overlay::ActionProgress(
                                    format!("Acquiring IP for {}...", target)
                                );
                            } else {
                                // Read error from log
                                let log_path = Config::cave_dir().join("deploy.log");
                                let err_msg = std::fs::read_to_string(&log_path)
                                    .ok()
                                    .and_then(|s| s.lines().last().map(|l| l.to_string()))
                                    .unwrap_or_else(|| "check ~/cave/deploy.log".to_string());
                                app.deploy_target = None;
                                app.deploy_wait_start = None;
                                app.overlay = Overlay::None;
                                app.set_status(&format!("Deploy failed: {}", err_msg));
                            }
                        }
                        Ok(None) => {
                            // Still running - update elapsed time
                            if let Some(start) = app.deploy_wait_start {
                                let secs = start.elapsed().as_secs();
                                let target = app.deploy_target.as_deref().unwrap_or("VM");
                                app.overlay = Overlay::ActionProgress(
                                    format!("Deploying to {}...\n\nElapsed: {}:{:02}", target, secs / 60, secs % 60)
                                );
                            }
                        }
                        Err(_) => {
                            app.deploy_handle = None;
                            app.overlay = Overlay::None;
                            app.set_status("Deploy error");
                        }
                    }
                }
            }

            // Check if waiting for VM IP (poll every tick while waiting)
            if app.deploy_waiting_for_ip {
                app.check_deploy_complete();
            }

            // Check if waiting for VM destroy confirmation
            if app.destroy_waiting {
                app.check_destroy_complete();
            }

            // Check if image download completed, update progress in overlay
            if app.image_download_handle.is_some() {
                if !app.check_download_complete() {
                    // Still downloading - update overlay with progress bar
                    if let Some(ref name) = app.image_download_name {
                        let progress = app.download_progress();
                        let elapsed = app.download_elapsed();
                        // Truncate name if too long
                        let short_name = if name.len() > 40 {
                            format!("{}...", &name[..37])
                        } else {
                            name.clone()
                        };
                        app.overlay = Overlay::ActionProgress(
                            format!("{}\n\n{}\n\nElapsed: {}", short_name, progress, elapsed)
                        );
                    }
                }
            }

            // Clear old status messages
            if let Some((_, created)) = &app.status_message {
                if created.elapsed() > Duration::from_secs(5) {
                    app.status_message = None;
                }
            }
        }

        // Continuous polling - check if poll finished, start new one if idle
        if app.check_poll_complete() {
            // Poll just finished and data refreshed, start another
            app.start_poll_if_idle();
        } else {
            // Start poll if none running
            app.start_poll_if_idle();
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
