use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{scan_for_ip, Config, Node};
use crate::ssh::SshConnection;
use crate::status::NodeStatus;
use crate::vm;

use super::ui;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Dashboard,
    Deploy,
    Images,
    Logs,
    Help,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeployStep {
    SelectNode,
    SelectImage,
    ConfigureVm,
    Confirm,
    Deploying,
    Complete,
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node: Node,
    pub ip: String,  // Current IP from network scan
    pub status: NodeStatus,
    pub cpu: String,
    pub cores: String,
    pub ram: String,
    pub vm_name: Option<String>,
    pub vm_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub name: String,
    pub size: u64,
}

pub struct App {
    pub running: bool,
    pub screen: Screen,
    pub config: Config,

    // Dashboard state
    pub nodes: Vec<NodeInfo>,
    pub images: Vec<ImageInfo>,
    pub selected_node_idx: usize,
    pub selected_image_idx: usize,
    pub server_running: bool,
    pub refreshing: bool,
    pub last_refresh: Instant,

    // Async refresh
    refresh_tx: Sender<RefreshRequest>,
    refresh_rx: Receiver<RefreshResult>,

    // Deploy wizard state
    pub deploy_step: DeployStep,
    pub deploy_node_idx: usize,
    pub deploy_image_idx: usize,
    pub deploy_vm_name: String,
    pub deploy_memory_idx: usize,
    pub deploy_cpu_idx: usize,
    pub deploy_disk_idx: usize,
    pub deploy_progress: f64,
    pub deploy_status: String,

    // UI state
    pub focus: Focus,
    pub show_popup: bool,
    pub popup_message: String,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    Nodes,
    Images,
    Actions,
}

pub const MEMORY_OPTIONS: &[(u32, &str)] = &[
    (1024, "1 GB"),
    (2048, "2 GB"),
    (4096, "4 GB"),
    (8192, "8 GB"),
    (16384, "16 GB"),
];

pub const CPU_OPTIONS: &[(u32, &str)] = &[
    (1, "1 CPU"),
    (2, "2 CPUs"),
    (4, "4 CPUs"),
    (8, "8 CPUs"),
];

pub const DISK_OPTIONS: &[(Option<u32>, &str)] = &[
    (None, "Default"),
    (Some(10), "10 GB"),
    (Some(20), "20 GB"),
    (Some(50), "50 GB"),
    (Some(100), "100 GB"),
    (Some(200), "200 GB"),
];

// Messages for async refresh
enum RefreshRequest {
    RefreshAll,
}

enum RefreshResult {
    NodesUpdated(Vec<NodeInfo>),
    ImagesUpdated(Vec<ImageInfo>),
    Done,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = Config::load()?;

        // Create channels for async refresh
        let (req_tx, req_rx) = mpsc::channel::<RefreshRequest>();
        let (res_tx, res_rx) = mpsc::channel::<RefreshResult>();

        // Spawn background refresh thread
        let config_clone = config.clone();
        thread::spawn(move || {
            refresh_worker(req_rx, res_tx, config_clone);
        });

        Ok(Self {
            running: true,
            screen: Screen::Dashboard,
            config,
            nodes: Vec::new(),
            images: Vec::new(),
            selected_node_idx: 0,
            selected_image_idx: 0,
            server_running: false,
            refreshing: false,
            last_refresh: Instant::now() - Duration::from_secs(60),
            refresh_tx: req_tx,
            refresh_rx: res_rx,
            deploy_step: DeployStep::SelectNode,
            deploy_node_idx: 0,
            deploy_image_idx: 0,
            deploy_vm_name: String::new(),
            deploy_memory_idx: 1,
            deploy_cpu_idx: 1,
            deploy_disk_idx: 2, // Default to 20GB
            deploy_progress: 0.0,
            deploy_status: String::new(),
            focus: Focus::Nodes,
            show_popup: false,
            popup_message: String::new(),
            logs: vec![
                "Welcome to Cave TUI".to_string(),
                "Press 'r' to refresh node status".to_string(),
                "Press 'd' to deploy a VM".to_string(),
                "Press '?' for help".to_string(),
            ],
        })
    }

    pub fn start_refresh(&mut self) {
        if !self.refreshing {
            self.refreshing = true;
            self.logs.push("Refreshing...".to_string());
            let _ = self.refresh_tx.send(RefreshRequest::RefreshAll);
        }
    }

    pub fn tick(&mut self) {
        // Check for refresh results (non-blocking)
        while let Ok(result) = self.refresh_rx.try_recv() {
            match result {
                RefreshResult::NodesUpdated(nodes) => {
                    self.nodes = nodes;
                }
                RefreshResult::ImagesUpdated(images) => {
                    self.images = images;
                }
                RefreshResult::Done => {
                    self.refreshing = false;
                    self.last_refresh = Instant::now();

                    // Check server status (quick, local check)
                    let pid_file = Config::pixiecore_pid_file();
                    self.server_running = pid_file.exists();

                    self.logs.push(format!(
                        "Refreshed: {} nodes, {} images",
                        self.nodes.len(),
                        self.images.len()
                    ));
                }
            }
        }

        // Auto-refresh every 30 seconds
        if !self.refreshing && self.last_refresh.elapsed() > Duration::from_secs(30) {
            self.start_refresh();
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        match self.screen {
            Screen::Dashboard => self.handle_dashboard_key(key),
            Screen::Deploy => self.handle_deploy_key(key),
            Screen::Images => self.handle_images_key(key),
            Screen::Help => self.handle_help_key(key),
            Screen::Logs => self.handle_logs_key(key),
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('r') => self.start_refresh(),
            KeyCode::Char('d') => {
                if !self.nodes.is_empty() && !self.images.is_empty() {
                    self.screen = Screen::Deploy;
                    self.deploy_step = DeployStep::SelectNode;
                    self.deploy_node_idx = self.selected_node_idx;
                    self.deploy_image_idx = 0;
                    if !self.nodes.is_empty() {
                        self.deploy_vm_name = self.nodes[self.deploy_node_idx].node.hostname.clone();
                    }
                } else {
                    self.show_popup = true;
                    self.popup_message = "Need at least one node and one image to deploy".to_string();
                }
            }
            KeyCode::Char('i') => self.screen = Screen::Images,
            KeyCode::Char('l') => self.screen = Screen::Logs,
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Nodes => Focus::Images,
                    Focus::Images => Focus::Actions,
                    Focus::Actions => Focus::Nodes,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.focus {
                    Focus::Nodes if !self.nodes.is_empty() => {
                        self.selected_node_idx = self.selected_node_idx.saturating_sub(1);
                    }
                    Focus::Images if !self.images.is_empty() => {
                        self.selected_image_idx = self.selected_image_idx.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.focus {
                    Focus::Nodes if !self.nodes.is_empty() => {
                        self.selected_node_idx = (self.selected_node_idx + 1).min(self.nodes.len() - 1);
                    }
                    Focus::Images if !self.images.is_empty() => {
                        self.selected_image_idx = (self.selected_image_idx + 1).min(self.images.len() - 1);
                    }
                    _ => {}
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.show_popup {
                    self.show_popup = false;
                }
            }
            KeyCode::Esc => {
                if self.show_popup {
                    self.show_popup = false;
                }
            }
            _ => {}
        }
    }

    fn handle_deploy_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => {
                self.screen = Screen::Dashboard;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.deploy_step {
                    DeployStep::SelectNode if !self.nodes.is_empty() => {
                        self.deploy_node_idx = self.deploy_node_idx.saturating_sub(1);
                        self.deploy_vm_name = self.nodes[self.deploy_node_idx].node.hostname.clone();
                    }
                    DeployStep::SelectImage if !self.images.is_empty() => {
                        self.deploy_image_idx = self.deploy_image_idx.saturating_sub(1);
                    }
                    DeployStep::ConfigureVm => {
                        // Toggle between memory and cpu selection
                    }
                    _ => {}
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.deploy_step {
                    DeployStep::SelectNode if !self.nodes.is_empty() => {
                        self.deploy_node_idx = (self.deploy_node_idx + 1).min(self.nodes.len() - 1);
                        self.deploy_vm_name = self.nodes[self.deploy_node_idx].node.hostname.clone();
                    }
                    DeployStep::SelectImage if !self.images.is_empty() => {
                        self.deploy_image_idx = (self.deploy_image_idx + 1).min(self.images.len() - 1);
                    }
                    _ => {}
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                match self.deploy_step {
                    DeployStep::ConfigureVm => {
                        self.deploy_memory_idx = self.deploy_memory_idx.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                match self.deploy_step {
                    DeployStep::ConfigureVm => {
                        self.deploy_memory_idx = (self.deploy_memory_idx + 1).min(MEMORY_OPTIONS.len() - 1);
                    }
                    _ => {}
                }
            }
            KeyCode::Tab => {
                match self.deploy_step {
                    DeployStep::ConfigureVm => {
                        // Cycle: CPU -> Disk -> CPU
                        self.deploy_cpu_idx = (self.deploy_cpu_idx + 1) % CPU_OPTIONS.len();
                    }
                    _ => {}
                }
            }
            KeyCode::Char('[') => {
                match self.deploy_step {
                    DeployStep::ConfigureVm => {
                        self.deploy_disk_idx = self.deploy_disk_idx.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            KeyCode::Char(']') => {
                match self.deploy_step {
                    DeployStep::ConfigureVm => {
                        self.deploy_disk_idx = (self.deploy_disk_idx + 1).min(DISK_OPTIONS.len() - 1);
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {
                match self.deploy_step {
                    DeployStep::SelectNode => {
                        self.deploy_step = DeployStep::SelectImage;
                    }
                    DeployStep::SelectImage => {
                        self.deploy_step = DeployStep::ConfigureVm;
                    }
                    DeployStep::ConfigureVm => {
                        self.deploy_step = DeployStep::Confirm;
                    }
                    DeployStep::Confirm => {
                        self.deploy_step = DeployStep::Deploying;
                        self.deploy_progress = 0.0;
                        self.deploy_status = "Starting deployment...".to_string();
                    }
                    DeployStep::Deploying => {
                        self.deploy_progress += 0.1;
                        if self.deploy_progress >= 1.0 {
                            self.deploy_step = DeployStep::Complete;
                        }
                    }
                    DeployStep::Complete => {
                        self.screen = Screen::Dashboard;
                        self.start_refresh();
                    }
                }
            }
            KeyCode::Backspace => {
                match self.deploy_step {
                    DeployStep::SelectImage => {
                        self.deploy_step = DeployStep::SelectNode;
                    }
                    DeployStep::ConfigureVm => {
                        self.deploy_step = DeployStep::SelectImage;
                    }
                    DeployStep::Confirm => {
                        self.deploy_step = DeployStep::ConfigureVm;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_images_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Dashboard,
            KeyCode::Up | KeyCode::Char('k') if !self.images.is_empty() => {
                self.selected_image_idx = self.selected_image_idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if !self.images.is_empty() => {
                self.selected_image_idx = (self.selected_image_idx + 1).min(self.images.len() - 1);
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

    fn handle_logs_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => {
                self.screen = Screen::Dashboard;
            }
            _ => {}
        }
    }
}

// Background refresh worker
fn refresh_worker(rx: Receiver<RefreshRequest>, tx: Sender<RefreshResult>, config: Config) {
    while let Ok(request) = rx.recv() {
        match request {
            RefreshRequest::RefreshAll => {
                // Refresh nodes in parallel using threads
                let nodes: Vec<NodeInfo> = config
                    .nodes
                    .iter()
                    .map(|node| {
                        let node = node.clone();
                        thread::spawn(move || get_node_info_fast(&node))
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .filter_map(|h| h.join().ok())
                    .collect();

                let _ = tx.send(RefreshResult::NodesUpdated(nodes));

                // Refresh images (fast, local filesystem)
                let images = get_images();
                let _ = tx.send(RefreshResult::ImagesUpdated(images));

                let _ = tx.send(RefreshResult::Done);
            }
        }
    }
}

// Fast node info with 2-second timeout
fn get_node_info_fast(node: &Node) -> NodeInfo {
    let timeout = Duration::from_secs(2);

    // Scan for IP first
    let ip = match scan_for_ip(&node.mac) {
        Some(ip) => ip,
        None => {
            return NodeInfo {
                node: node.clone(),
                ip: String::new(),
                status: NodeStatus::Offline,
                cpu: String::new(),
                cores: String::new(),
                ram: String::new(),
                vm_name: None,
                vm_ip: None,
            };
        }
    };

    // Try to connect with short timeout
    let ssh = SshConnection::connect_timeout(&ip, timeout);

    match ssh {
        Ok(ssh) => {
            // Check if VM is running
            let (status, vm_name, vm_ip) = check_vm_status(&ssh, &ip);

            // Get basic specs (quick command)
            let (cpu, cores, ram) = get_specs_fast(&ssh);

            NodeInfo {
                node: node.clone(),
                ip,
                status,
                cpu,
                cores,
                ram,
                vm_name,
                vm_ip,
            }
        }
        Err(_) => NodeInfo {
            node: node.clone(),
            ip,
            status: NodeStatus::Offline,
            cpu: String::new(),
            cores: String::new(),
            ram: String::new(),
            vm_name: None,
            vm_ip: None,
        },
    }
}

fn check_vm_status(ssh: &SshConnection, _ip: &str) -> (NodeStatus, Option<String>, Option<String>) {
    let output = ssh.execute(&format!(
        r#"for pid in {}/*.pid; do
            [ -f "$pid" ] && kill -0 $(cat "$pid") 2>/dev/null && {{
                vm=$(basename "$pid" .pid)
                ip=$(grep 'ci-info:.*ens.*True' "{}/$vm.log" 2>/dev/null | sed 's/.*True[^0-9]*\([0-9.]\+\).*/\1/' | head -1)
                echo "$vm|$ip"
                exit 0
            }}
        done"#,
        vm::VM_RUN_PATH, vm::VM_RUN_PATH
    ));

    match output {
        Ok(out) => {
            let parts: Vec<&str> = out.trim().split('|').collect();
            if parts.len() >= 1 && !parts[0].is_empty() {
                let vm_name = Some(parts[0].to_string());
                let vm_ip = if parts.len() >= 2 && !parts[1].is_empty() {
                    Some(parts[1].to_string())
                } else {
                    None
                };
                (NodeStatus::Active, vm_name, vm_ip)
            } else {
                (NodeStatus::Standby, None, None)
            }
        }
        Err(_) => (NodeStatus::Standby, None, None),
    }
}

fn get_specs_fast(ssh: &SshConnection) -> (String, String, String) {
    let output = ssh.execute(
        r#"echo "$(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | xargs)|$(nproc)|$(free -h | awk '/^Mem:/ {print $2}')""#
    );

    match output {
        Ok(out) => {
            let parts: Vec<&str> = out.trim().split('|').collect();
            if parts.len() >= 3 {
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                )
            } else {
                (String::new(), String::new(), String::new())
            }
        }
        Err(_) => (String::new(), String::new(), String::new()),
    }
}

fn get_images() -> Vec<ImageInfo> {
    let mut images = Vec::new();
    if let Ok(entries) = std::fs::read_dir(Config::images_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.ends_with("-seed.iso") && !name.ends_with("-seed") {
                        if let Ok(meta) = std::fs::metadata(&path) {
                            images.push(ImageInfo {
                                name: name.to_string(),
                                size: meta.len(),
                            });
                        }
                    }
                }
            }
        }
    }
    images.sort_by(|a, b| a.name.cmp(&b.name));
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
    app.start_refresh(); // Initial refresh

    let tick_rate = Duration::from_millis(50); // Faster tick for smoother UI
    let mut last_tick = Instant::now();

    loop {
        // Draw UI
        terminal.draw(|f| ui::draw(f, &app))?;

        // Handle input with short timeout
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        // Tick - check for async results
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
