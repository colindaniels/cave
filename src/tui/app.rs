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

use crate::config::{load_node_cache, CachedNode, Config};
use crate::commands::images::{get_image_display_name, CloudImage, CLOUD_IMAGES};

use super::ui;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Dashboard,
    NodeDetails,
    Images,
    ImageDownload,
    Help,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub filename: String,
    pub display_name: String,
    pub size: u64,
}

pub struct App {
    pub running: bool,
    pub screen: Screen,

    // Data
    pub nodes: Vec<CachedNode>,
    pub images: Vec<ImageInfo>,
    pub server_running: bool,

    // Selection state
    pub selected_node_idx: usize,
    pub selected_image_idx: usize,
    pub selected_action_idx: usize,

    // Image search (local)
    pub image_search: String,
    pub image_search_active: bool,

    // Cloud image search (download)
    pub cloud_search: String,
    pub cloud_search_idx: usize,

    // Refresh
    pub last_refresh: Instant,

    // Status messages
    pub status_message: Option<(String, Instant)>,
}

pub const NODE_ACTIONS: &[(&str, &str, &str)] = &[
    ("deploy", "Deploy VM", "Deploy a VM image to this node"),
    ("destroy", "Destroy VM", "Stop and remove the VM"),
    ("wake", "Wake", "Send Wake-on-LAN packet"),
    ("shutdown", "Shutdown", "Gracefully power off"),
    ("restart", "Restart", "Shutdown and wake"),
    ("remove", "Remove", "Unregister this node"),
];

impl App {
    pub fn new() -> Result<Self> {
        let nodes = load_node_cache();
        let images = get_images();

        // Check server status
        let pid_file = Config::pixiecore_pid_file();
        let server_running = pid_file.exists();

        Ok(Self {
            running: true,
            screen: Screen::Dashboard,
            nodes,
            images,
            server_running,
            selected_node_idx: 0,
            selected_image_idx: 0,
            selected_action_idx: 0,
            image_search: String::new(),
            image_search_active: false,
            cloud_search: String::new(),
            cloud_search_idx: 0,
            last_refresh: Instant::now(),
            status_message: None,
        })
    }

    pub fn refresh(&mut self) {
        self.nodes = load_node_cache();
        self.images = get_images();
        let pid_file = Config::pixiecore_pid_file();
        self.server_running = pid_file.exists();
        self.last_refresh = Instant::now();
        self.set_status("Refreshed");
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), Instant::now()));
    }

    pub fn tick(&mut self) {
        // Clear status after 3 seconds
        if let Some((_, time)) = &self.status_message {
            if time.elapsed() > Duration::from_secs(3) {
                self.status_message = None;
            }
        }

        // Auto-refresh every 10 seconds
        if self.last_refresh.elapsed() > Duration::from_secs(10) {
            self.refresh();
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        match self.screen {
            Screen::Dashboard => self.handle_dashboard_key(key),
            Screen::NodeDetails => self.handle_node_details_key(key),
            Screen::Images => self.handle_images_key(key),
            Screen::ImageDownload => self.handle_image_download_key(key),
            Screen::Help => self.handle_help_key(key),
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('i') => self.screen = Screen::Images,
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.nodes.is_empty() {
                    self.selected_node_idx = self.selected_node_idx.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.nodes.is_empty() {
                    self.selected_node_idx = (self.selected_node_idx + 1).min(self.nodes.len() - 1);
                }
            }
            KeyCode::Enter => {
                if !self.nodes.is_empty() {
                    self.selected_action_idx = 0;
                    self.screen = Screen::NodeDetails;
                }
            }
            _ => {}
        }
    }

    fn handle_node_details_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Dashboard,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_action_idx = self.selected_action_idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_action_idx = (self.selected_action_idx + 1).min(NODE_ACTIONS.len() - 1);
            }
            KeyCode::Enter => {
                self.execute_action();
            }
            // Quick action keys
            KeyCode::Char('w') => {
                self.selected_action_idx = 2; // wake
                self.execute_action();
            }
            KeyCode::Char('s') => {
                self.selected_action_idx = 3; // shutdown
                self.execute_action();
            }
            KeyCode::Char('d') => {
                self.selected_action_idx = 0; // deploy
                self.execute_action();
            }
            _ => {}
        }
    }

    fn handle_images_key(&mut self, key: KeyCode) {
        if self.image_search_active {
            // Search mode - handle typing
            match key {
                KeyCode::Esc => {
                    self.image_search_active = false;
                }
                KeyCode::Enter => {
                    self.image_search_active = false;
                }
                KeyCode::Backspace => {
                    self.image_search.pop();
                    self.selected_image_idx = 0;
                }
                KeyCode::Char(c) => {
                    self.image_search.push(c);
                    self.selected_image_idx = 0;
                }
                KeyCode::Up => {
                    let filtered = self.filtered_images();
                    if !filtered.is_empty() {
                        self.selected_image_idx = self.selected_image_idx.saturating_sub(1);
                    }
                }
                KeyCode::Down => {
                    let filtered = self.filtered_images();
                    if !filtered.is_empty() {
                        self.selected_image_idx = (self.selected_image_idx + 1).min(filtered.len() - 1);
                    }
                }
                _ => {}
            }
        } else {
            // Normal mode
            match key {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.image_search.clear();
                    self.screen = Screen::Dashboard;
                }
                KeyCode::Char('/') => {
                    self.image_search_active = true;
                }
                KeyCode::Char('d') => {
                    // Go to download screen
                    self.cloud_search.clear();
                    self.cloud_search_idx = 0;
                    self.screen = Screen::ImageDownload;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let filtered = self.filtered_images();
                    if !filtered.is_empty() {
                        self.selected_image_idx = self.selected_image_idx.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let filtered = self.filtered_images();
                    if !filtered.is_empty() {
                        self.selected_image_idx = (self.selected_image_idx + 1).min(filtered.len() - 1);
                    }
                }
                KeyCode::Backspace => {
                    // Clear search with backspace when not in search mode
                    if !self.image_search.is_empty() {
                        self.image_search.pop();
                        self.selected_image_idx = 0;
                    }
                }
                KeyCode::Char(c) if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' => {
                    // Quick search - start typing to filter
                    self.image_search.push(c);
                    self.selected_image_idx = 0;
                }
                _ => {}
            }
        }
    }

    fn handle_image_download_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.screen = Screen::Images;
            }
            KeyCode::Backspace => {
                self.cloud_search.pop();
                self.cloud_search_idx = 0;
            }
            KeyCode::Up => {
                let filtered = self.filtered_cloud_images();
                if !filtered.is_empty() {
                    self.cloud_search_idx = self.cloud_search_idx.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                let filtered = self.filtered_cloud_images();
                if !filtered.is_empty() {
                    self.cloud_search_idx = (self.cloud_search_idx + 1).min(filtered.len() - 1);
                }
            }
            KeyCode::Enter => {
                self.download_selected_cloud_image();
            }
            KeyCode::Char(c) => {
                // All characters go to search (use arrow keys to navigate)
                self.cloud_search.push(c);
                self.cloud_search_idx = 0;
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.screen = Screen::Dashboard;
            }
            _ => {}
        }
    }

    fn execute_action(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        let node = &self.nodes[self.selected_node_idx];
        let hostname = &node.hostname;
        let action = NODE_ACTIONS[self.selected_action_idx].0;

        // Execute the cave command
        let result = match action {
            "wake" => Command::new("cave")
                .args(["node", "wake", hostname])
                .output(),
            "shutdown" => Command::new("cave")
                .args(["node", "shutdown", hostname])
                .output(),
            "restart" => Command::new("cave")
                .args(["node", "restart", hostname])
                .output(),
            "destroy" => Command::new("cave")
                .args(["node", "destroy", hostname])
                .output(),
            "remove" => {
                Command::new("cave")
                    .args(["node", "remove", hostname])
                    .output()
            }
            "deploy" => {
                // For deploy, we need to exit TUI and run interactively
                self.set_status("Use CLI for deploy: cave node deploy");
                return;
            }
            _ => return,
        };

        match result {
            Ok(output) => {
                if output.status.success() {
                    self.set_status(&format!("{} executed on {}", action, hostname));
                    // Refresh after action
                    std::thread::sleep(Duration::from_millis(500));
                    self.refresh();
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    self.set_status(&format!("Failed: {}", err.lines().next().unwrap_or("error")));
                }
            }
            Err(e) => {
                self.set_status(&format!("Error: {}", e));
            }
        }
    }

    pub fn selected_node(&self) -> Option<&CachedNode> {
        self.nodes.get(self.selected_node_idx)
    }

    pub fn filtered_images(&self) -> Vec<&ImageInfo> {
        if self.image_search.is_empty() {
            self.images.iter().collect()
        } else {
            let search = self.image_search.to_lowercase();
            self.images
                .iter()
                .filter(|img| {
                    img.display_name.to_lowercase().contains(&search)
                        || img.filename.to_lowercase().contains(&search)
                })
                .collect()
        }
    }

    pub fn filtered_cloud_images(&self) -> Vec<&'static CloudImage> {
        if self.cloud_search.is_empty() {
            CLOUD_IMAGES.iter().collect()
        } else {
            let search = self.cloud_search.to_lowercase();
            CLOUD_IMAGES
                .iter()
                .filter(|img| {
                    img.name.to_lowercase().contains(&search)
                        || img.version.to_lowercase().contains(&search)
                })
                .collect()
        }
    }

    fn download_selected_cloud_image(&mut self) {
        let filtered = self.filtered_cloud_images();
        if filtered.is_empty() {
            return;
        }

        let img = filtered[self.cloud_search_idx];
        let url = img.url;

        // Run cave image pull in background
        self.set_status(&format!("Downloading {} {}...", img.name, img.version));

        let result = Command::new("cave")
            .args(["image", "pull", url])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    self.set_status(&format!("Downloaded {} {}", img.name, img.version));
                    self.refresh();
                    self.screen = Screen::Images;
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    self.set_status(&format!("Failed: {}", err.lines().next().unwrap_or("error")));
                }
            }
            Err(e) => {
                self.set_status(&format!("Error: {}", e));
            }
        }
    }
}

fn get_images() -> Vec<ImageInfo> {
    let mut images = Vec::new();
    if let Ok(entries) = std::fs::read_dir(Config::images_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Skip seed ISOs and other non-image files
                    if name.ends_with("-seed.iso")
                        || name.ends_with("-seed")
                        || name.contains(".cave.")
                    {
                        continue;
                    }
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let display_name = get_image_display_name(name);
                        images.push(ImageInfo {
                            filename: name.to_string(),
                            display_name,
                            size: meta.len(),
                        });
                    }
                }
            }
        }
    }
    images.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    images
}

pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new()?;

    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw(f, &app))?;

        // Handle input
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        // Tick
        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
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
