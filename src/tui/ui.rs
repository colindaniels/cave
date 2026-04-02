use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Clear, List, ListItem, Paragraph},
    Frame,
};

use super::app::{
    App, DeployStep, Overlay, CPU_OPTIONS, DISCOVERED_NODE_ACTIONS, MEMORY_OPTIONS, NODE_ACTIONS,
};
use super::widgets::logo::LOGO;
use crate::commands::images::CLOUD_IMAGES;

// ============================================================================
// Color Scheme - Catppuccin Mocha
// ============================================================================

const BASE: Color = Color::Rgb(30, 30, 46);
const SURFACE0: Color = Color::Rgb(49, 50, 68);
const SURFACE1: Color = Color::Rgb(69, 71, 90);
const TEXT: Color = Color::Rgb(205, 214, 244);
const SUBTEXT: Color = Color::Rgb(166, 173, 200);
const GREEN: Color = Color::Rgb(166, 227, 161);
const YELLOW: Color = Color::Rgb(249, 226, 175);
const RED: Color = Color::Rgb(243, 139, 168);
const BLUE: Color = Color::Rgb(137, 180, 250);
const MAUVE: Color = Color::Rgb(203, 166, 247);
const LAVENDER: Color = Color::Rgb(180, 190, 254);

// ============================================================================
// Main Draw
// ============================================================================

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();

    // Fill background
    f.render_widget(Block::default().style(Style::default().bg(BASE)), size);

    // Main vertical layout: logo + stats bar + content area + status bar
    let main_vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Logo (full width)
            Constraint::Length(5),  // Stats bar (full width)
            Constraint::Min(10),    // Content area (nodes + details + server panel)
            Constraint::Length(1),  // Status bar (full width)
        ])
        .split(size);

    draw_logo(f, main_vertical[0]);
    draw_stats_bar(f, app, main_vertical[1]);
    draw_status_bar(f, app, main_vertical[3]);

    // Content area: split horizontally into main content and server panel
    let content_horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(50),      // Left: nodes + details
            Constraint::Length(32),   // Right: server panel
        ])
        .split(main_vertical[2]);

    draw_main_content(f, app, content_horizontal[0]);
    draw_server_panel(f, app, content_horizontal[1]);

    // Draw overlays on top
    match &app.overlay {
        Overlay::None => {}
        Overlay::NodeActions => draw_node_actions_overlay(f, app),
        Overlay::Deploy(step) => draw_deploy_overlay(f, app, step.clone()),
        Overlay::ActionProgress(msg) => draw_action_progress_overlay(f, msg),
        Overlay::ImageDownload => draw_image_download_overlay(f, app),
        Overlay::NodeInit => draw_node_init_overlay(f, app),
        Overlay::ConfirmRemove => draw_confirm_remove_overlay(f, app),
        Overlay::Help => draw_help_overlay(f),
    }
}

// ============================================================================
// Logo
// ============================================================================

fn draw_logo(f: &mut Frame, area: Rect) {
    let logo_lines: Vec<Line> = LOGO
        .lines()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(MAUVE))))
        .collect();

    let logo = Paragraph::new(logo_lines)
        .alignment(Alignment::Center)
        .style(Style::default().bg(BASE));

    f.render_widget(logo, area);
}

// ============================================================================
// Top Stats Bar
// ============================================================================

fn draw_stats_bar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Cluster Overview ")
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE1))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Count nodes by status
    let online = app.nodes.iter().filter(|n| n.status == "active").count();
    let standby = app.nodes.iter().filter(|n| n.status == "standby").count();
    let offline = app.nodes.iter().filter(|n| n.status == "offline").count();
    let with_vm = app.nodes.iter().filter(|n| n.vm.is_some()).count();

    // Calculate total disk from all nodes
    let total_disk: u64 = app.nodes.iter()
        .flat_map(|n| n.disks.iter())
        .map(|d| d.size_bytes)
        .sum();

    let stats_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(inner);

    // Nodes stat
    let nodes_text = vec![
        Line::from(vec![
            Span::styled(format!("{}", online), Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" online ", Style::default().fg(SUBTEXT)),
            Span::styled(format!("{}", standby), Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" standby ", Style::default().fg(SUBTEXT)),
            Span::styled(format!("{}", offline), Style::default().fg(if offline > 0 { RED } else { SUBTEXT }).add_modifier(Modifier::BOLD)),
            Span::styled(" off", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!("{} VMs running", with_vm), Style::default().fg(SUBTEXT)),
        ]),
    ];
    let nodes_para = Paragraph::new(nodes_text)
        .block(Block::default().title("Nodes").title_style(Style::default().fg(TEXT)))
        .alignment(Alignment::Center);
    f.render_widget(nodes_para, stats_layout[0]);

    // Total cores
    let total_cores: u32 = app.nodes.iter()
        .filter_map(|n| n.cores.parse::<u32>().ok())
        .sum();
    let cores_text = vec![
        Line::from(vec![
            Span::styled(format!("{}", total_cores), Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(" cores", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled("total capacity", Style::default().fg(SUBTEXT)),
        ]),
    ];
    let cores_para = Paragraph::new(cores_text)
        .block(Block::default().title("CPU").title_style(Style::default().fg(TEXT)))
        .alignment(Alignment::Center);
    f.render_widget(cores_para, stats_layout[1]);

    // Disk stat
    let disk_text = vec![
        Line::from(vec![
            Span::styled(format_size(total_disk), Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("total capacity", Style::default().fg(SUBTEXT)),
        ]),
    ];
    let disk_para = Paragraph::new(disk_text)
        .block(Block::default().title("Storage").title_style(Style::default().fg(TEXT)))
        .alignment(Alignment::Center);
    f.render_widget(disk_para, stats_layout[2]);

    // Images stat
    let images_text = vec![
        Line::from(vec![
            Span::styled(format!("{}", app.local_images.len()), Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(" local", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!("{} available", CLOUD_IMAGES.len()), Style::default().fg(SUBTEXT)),
        ]),
    ];
    let images_para = Paragraph::new(images_text)
        .block(Block::default().title("Images").title_style(Style::default().fg(TEXT)))
        .alignment(Alignment::Center);
    f.render_widget(images_para, stats_layout[3]);
}

// ============================================================================
// Main Content (Node List + Node Details)
// ============================================================================

fn draw_main_content(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),  // Node list
            Constraint::Percentage(70),  // Node details
        ])
        .split(area);

    draw_node_list(f, app, chunks[0]);
    draw_node_details(f, app, chunks[1]);
}

fn draw_node_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Nodes ")
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE1))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let total = app.nodes.len() + app.discovered_nodes.len();
    if total == 0 {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("No nodes", Style::default().fg(SUBTEXT))),
            Line::from(""),
            Line::from(Span::styled("Press 'n' to add one", Style::default().fg(SUBTEXT))),
        ])
        .alignment(Alignment::Center);
        f.render_widget(empty, inner);
        return;
    }

    let mut items: Vec<ListItem> = app
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let (indicator, color) = if node.vm.is_some() {
                ("●", GREEN)  // Green: VM running
            } else if node.status == "active" || node.status == "standby" {
                ("●", YELLOW) // Yellow: online/standby but no VM
            } else {
                ("●", RED)    // Red: offline
            };

            let style = if i == app.selected_node_idx {
                Style::default().fg(TEXT).bg(SURFACE0).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled(format!(" {} ", indicator), Style::default().fg(color)),
                    Span::styled(&node.hostname, style),
                ]),
            ];

            // Add VM name indented below if present
            if let Some(ref vm) = node.vm {
                lines.push(Line::from(vec![
                    Span::styled("   └─ ", Style::default().fg(SURFACE1)),
                    Span::styled(&vm.name, Style::default().fg(MAUVE)),
                ]));
            }

            ListItem::new(lines).style(style)
        })
        .collect();

    // Append discovered (uninitialized) nodes
    for (i, disc) in app.discovered_nodes.iter().enumerate() {
        let idx = app.nodes.len() + i;
        let style = if idx == app.selected_node_idx {
            Style::default().fg(TEXT).bg(SURFACE0).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(SUBTEXT)
        };

        items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(" ○ ", Style::default().fg(BLUE)),
                Span::styled(&disc.mac, style),
            ]),
        ]).style(style));
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_node_details(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Node Details ")
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE1))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Get selected node - could be registered or discovered
    let node = if app.selected_node_idx < app.nodes.len() {
        app.selected_node()
    } else {
        app.selected_discovered()
    };

    let Some(node) = node else {
        let empty = Paragraph::new("Select a node")
            .style(Style::default().fg(SUBTEXT))
            .alignment(Alignment::Center);
        f.render_widget(empty, inner);
        return;
    };

    let is_discovered = app.is_selected_discovered();

    // Split into info area and actions
    let detail_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),      // Node info
            Constraint::Length(5),   // Actions hint
        ])
        .split(inner);

    // Node status (the node itself, not VM) - colored circle + plain text
    let (node_status_color, node_status_text) = if node.status == "active" {
        (GREEN, "Online")
    } else if node.status == "standby" {
        (YELLOW, "Standby")
    } else {
        (RED, "Offline")
    };

    let ip_display = node.ip.as_deref().unwrap_or("-");
    let cpu_display = format!("{} ({} cores)", &node.cpu, &node.cores);
    let total_disk: u64 = node.disks.iter().map(|d| d.size_bytes).sum();
    let disk_display = format_size(total_disk);

    // RAM display: show usage if available, otherwise just total
    let (ram_display, ram_color) = match (node.ram_used_mb, node.ram_total_mb) {
        (Some(used), Some(total)) => {
            let (display, percent) = format_ram_usage(used, total);
            (display, usage_color(percent))
        }
        _ => (node.ram.clone(), TEXT), // Fallback to the raw string like "7.5G"
    };

    let (section_title, name_display) = if is_discovered {
        ("  ── Discovered Node ──", "Not initialized".to_string())
    } else {
        ("  ── Node ──", node.hostname.clone())
    };

    let mut info_lines = vec![
        // Node section header
        Line::from(vec![
            Span::styled(section_title, Style::default().fg(if is_discovered { BLUE } else { LAVENDER }).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Name     ", Style::default().fg(SUBTEXT)),
            Span::styled(name_display, Style::default().fg(if is_discovered { BLUE } else { TEXT }).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Status   ", Style::default().fg(SUBTEXT)),
            Span::styled("● ", Style::default().fg(node_status_color)),
            Span::styled(node_status_text, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  IP       ", Style::default().fg(SUBTEXT)),
            Span::styled(ip_display, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  MAC      ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.mac, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  CPU      ", Style::default().fg(SUBTEXT)),
            Span::styled(&cpu_display, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  RAM      ", Style::default().fg(SUBTEXT)),
            Span::styled(&ram_display, Style::default().fg(ram_color)),
        ]),
        Line::from(vec![
            Span::styled("  Storage  ", Style::default().fg(SUBTEXT)),
            Span::styled(&disk_display, Style::default().fg(TEXT)),
        ]),
    ];

    // Add VM section if running
    if let Some(ref vm) = node.vm {
        info_lines.push(Line::from(""));
        info_lines.push(Line::from(vec![
            Span::styled("  ── VM ──", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  Name     ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.name, Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        ]));
        // VM status - if we have VM info, it's online/running
        let (vm_status_color, vm_status_text) = if vm.ip.is_empty() {
            (YELLOW, "Starting")
        } else {
            (GREEN, "Online")
        };
        info_lines.push(Line::from(vec![
            Span::styled("  Status   ", Style::default().fg(SUBTEXT)),
            Span::styled("● ", Style::default().fg(vm_status_color)),
            Span::styled(vm_status_text, Style::default().fg(TEXT)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  IP       ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.ip, Style::default().fg(TEXT)),
        ]));

        // VM Memory display: show usage if available
        let (vm_mem_display, vm_mem_percent) = format_vm_memory_usage(vm);
        let vm_mem_color = vm_mem_percent.map(usage_color).unwrap_or(TEXT);
        info_lines.push(Line::from(vec![
            Span::styled("  Memory   ", Style::default().fg(SUBTEXT)),
            Span::styled(vm_mem_display, Style::default().fg(vm_mem_color)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  CPUs     ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.cpus, Style::default().fg(TEXT)),
        ]));
    } else {
        info_lines.push(Line::from(""));
        info_lines.push(Line::from(vec![
            Span::styled("  ── VM ──", Style::default().fg(SUBTEXT)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  No VM deployed", Style::default().fg(SUBTEXT)),
        ]));
    }

    let info_para = Paragraph::new(info_lines);
    f.render_widget(info_para, detail_chunks[0]);

    // Actions hint
    let hint = Line::from(vec![
        Span::styled("Press ", Style::default().fg(SUBTEXT)),
        Span::styled("Enter", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" for actions", Style::default().fg(SUBTEXT)),
    ]);
    let actions_para = Paragraph::new(vec![Line::from(""), hint])
        .alignment(Alignment::Center);
    f.render_widget(actions_para, detail_chunks[1]);
}

// ============================================================================
// Status Bar
// ============================================================================

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let help_text = vec![
        Span::styled(" [j/k]", Style::default().fg(BLUE)),
        Span::styled(" navigate  ", Style::default().fg(SUBTEXT)),
        Span::styled("[Enter]", Style::default().fg(BLUE)),
        Span::styled(" actions  ", Style::default().fg(SUBTEXT)),
        Span::styled("[n]", Style::default().fg(BLUE)),
        Span::styled(" new node  ", Style::default().fg(SUBTEXT)),
        Span::styled("[i]", Style::default().fg(BLUE)),
        Span::styled(" images  ", Style::default().fg(SUBTEXT)),
        Span::styled("[?]", Style::default().fg(BLUE)),
        Span::styled(" help  ", Style::default().fg(SUBTEXT)),
        Span::styled("[q]", Style::default().fg(BLUE)),
        Span::styled(" quit", Style::default().fg(SUBTEXT)),
    ];

    let mut line = Line::from(help_text);

    // Add status message if present
    if let Some((msg, _)) = &app.status_message {
        line = Line::from(vec![
            Span::styled(format!(" {} ", msg), Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
        ]);
    }

    let para = Paragraph::new(line)
        .style(Style::default().bg(SURFACE0));
    f.render_widget(para, area);
}

// ============================================================================
// Overlays
// ============================================================================

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

fn draw_node_actions_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(40, 12, f.area());
    f.render_widget(Clear, area);

    let (node_name, actions) = if app.is_selected_discovered() {
        let name = app.selected_discovered()
            .map(|n| n.mac.as_str())
            .unwrap_or("Unknown");
        (name, DISCOVERED_NODE_ACTIONS)
    } else {
        let name = app.selected_node()
            .map(|n| n.hostname.as_str())
            .unwrap_or("Node");
        (name, NODE_ACTIONS)
    };

    let block = Block::default()
        .title(format!(" {} ", node_name))
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let style = if i == app.selected_action_idx {
                Style::default().fg(TEXT).bg(SURFACE0).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };
            ListItem::new(format!("  {}", action)).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_deploy_overlay(f: &mut Frame, app: &App, step: DeployStep) {
    let area = centered_rect(60, 20, f.area());
    f.render_widget(Clear, area);

    let title = match step {
        DeployStep::SelectImage => " Deploy: Select Image ",
        DeployStep::SelectDisk => " Deploy: Select Disk ",
        DeployStep::Configure => " Deploy: Configure VM ",
        DeployStep::Confirm => " Deploy: Confirm ",
        DeployStep::Deploying => " Deploying... ",
        DeployStep::Done => " Deploy Complete ",
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(MAUVE).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MAUVE))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    match step {
        DeployStep::SelectImage => {
            let images = app.filtered_images();

            let content_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // Search
                    Constraint::Min(5),     // List
                ])
                .split(inner);

            // Search field
            let search_text = if app.image_filter.is_empty() {
                Span::styled("Type to filter...", Style::default().fg(SUBTEXT))
            } else {
                Span::styled(&app.image_filter, Style::default().fg(TEXT))
            };
            let search = Paragraph::new(Line::from(vec![
                Span::styled(" Filter: ", Style::default().fg(SUBTEXT)),
                search_text,
                Span::styled("_", Style::default().fg(BLUE)),
            ]));
            f.render_widget(search, content_chunks[0]);

            // Image list
            if images.is_empty() {
                let empty = Paragraph::new("No images found")
                    .style(Style::default().fg(SUBTEXT))
                    .alignment(Alignment::Center);
                f.render_widget(empty, content_chunks[1]);
            } else {
                let items: Vec<ListItem> = images
                    .iter()
                    .enumerate()
                    .map(|(i, (name, size))| {
                        let style = if i == app.deploy_image_idx {
                            Style::default().fg(TEXT).bg(SURFACE0).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(TEXT)
                        };
                        let display = crate::commands::images::get_image_display_name(name);
                        ListItem::new(format!("  {} ({})", display, format_size(*size))).style(style)
                    })
                    .collect();

                let list = List::new(items);
                f.render_widget(list, content_chunks[1]);
            }
        }

        DeployStep::SelectDisk => {
            let node = app.selected_node();
            let disks = node.map(|n| &n.disks).cloned().unwrap_or_default();

            if disks.is_empty() {
                let empty = Paragraph::new("No disks found on this node")
                    .style(Style::default().fg(RED))
                    .alignment(Alignment::Center);
                f.render_widget(empty, inner);
            } else {
                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("Select disk for VM storage:", Style::default().fg(SUBTEXT))),
                    Line::from(""),
                ];

                for (i, disk) in disks.iter().enumerate() {
                    let size_display = format_size(disk.size_bytes);
                    let selected = i == app.deploy_disk_select_idx;
                    let style = if selected {
                        Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(TEXT)
                    };
                    let arrow = if selected { " > " } else { "   " };
                    // Show: /dev/nvme0n1  500 GB  (SSD, Samsung 980 Pro)
                    let model_info = if disk.model.is_empty() {
                        disk.disk_type.clone()
                    } else {
                        format!("{}, {}", disk.disk_type, disk.model)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(arrow, Style::default().fg(MAUVE)),
                        Span::styled(format!("/dev/{}", disk.name), style),
                        Span::styled(format!("  {}", size_display), Style::default().fg(TEXT)),
                        Span::styled(format!("  ({})", model_info), Style::default().fg(SUBTEXT)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("Press Enter to continue", Style::default().fg(SUBTEXT))));

                let para = Paragraph::new(lines).alignment(Alignment::Center);
                f.render_widget(para, inner);
            }
        }

        DeployStep::Configure => {
            let disk_label = app.selected_disk_size_label();
            let fields: [(&str, &str, bool); 3] = [
                ("Memory", MEMORY_OPTIONS[app.deploy_memory_idx].1, app.deploy_config_field == 0),
                ("CPUs", CPU_OPTIONS[app.deploy_cpu_idx].1, app.deploy_config_field == 1),
                ("Disk", &disk_label, app.deploy_config_field == 2),
            ];

            let mut lines = vec![Line::from("")];
            for (label, value, selected) in fields {
                let style = if selected {
                    Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                };
                let arrow = if selected { " > " } else { "   " };
                lines.push(Line::from(vec![
                    Span::styled(arrow, Style::default().fg(MAUVE)),
                    Span::styled(format!("{:<10}", label), Style::default().fg(SUBTEXT)),
                    Span::styled(format!(" < {} >", value), style),
                ]));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Use arrow keys to adjust, Enter to continue", Style::default().fg(SUBTEXT))));

            let para = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(para, inner);
        }

        DeployStep::Confirm => {
            let images = app.filtered_images();
            let image_name = images.get(app.deploy_image_idx)
                .map(|(n, _)| n.as_str())
                .unwrap_or("Unknown");
            let node_name = app.selected_node()
                .map(|n| n.hostname.as_str())
                .unwrap_or("Unknown");
            let disk_label = app.selected_disk_size_label();

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled("Deploy VM?", Style::default().fg(TEXT).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Node:   ", Style::default().fg(SUBTEXT)),
                    Span::styled(node_name, Style::default().fg(MAUVE)),
                ]),
                Line::from(vec![
                    Span::styled("  Image:  ", Style::default().fg(SUBTEXT)),
                    Span::styled(image_name, Style::default().fg(BLUE)),
                ]),
                Line::from(vec![
                    Span::styled("  Memory: ", Style::default().fg(SUBTEXT)),
                    Span::styled(MEMORY_OPTIONS[app.deploy_memory_idx].1, Style::default().fg(TEXT)),
                ]),
                Line::from(vec![
                    Span::styled("  CPUs:   ", Style::default().fg(SUBTEXT)),
                    Span::styled(CPU_OPTIONS[app.deploy_cpu_idx].1, Style::default().fg(TEXT)),
                ]),
                Line::from(vec![
                    Span::styled("  Disk:   ", Style::default().fg(SUBTEXT)),
                    Span::styled(disk_label, Style::default().fg(TEXT)),
                ]),
                Line::from(""),
                Line::from(""),
                Line::from(vec![
                    Span::styled("[y/Enter]", Style::default().fg(GREEN)),
                    Span::styled(" confirm  ", Style::default().fg(SUBTEXT)),
                    Span::styled("[n/Esc]", Style::default().fg(RED)),
                    Span::styled(" cancel", Style::default().fg(SUBTEXT)),
                ]),
            ];

            let para = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(para, inner);
        }

        DeployStep::Deploying => {
            let lines = vec![
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled("Deploying...", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(Span::styled("Please wait", Style::default().fg(SUBTEXT))),
            ];
            let para = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(para, inner);
        }

        DeployStep::Done => {
            let lines = vec![
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled("Done!", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(Span::styled("Press Enter to continue", Style::default().fg(SUBTEXT))),
            ];
            let para = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(para, inner);
        }
    }
}

fn draw_image_download_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(70, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Download Cloud Image ")
        .title_style(Style::default().fg(BLUE).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BLUE))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Search
            Constraint::Min(5),     // List
        ])
        .split(inner);

    // Search field
    let search_text = if app.cloud_search.is_empty() {
        Span::styled("Type to search (ubuntu, debian, arch...)", Style::default().fg(SUBTEXT))
    } else {
        Span::styled(&app.cloud_search, Style::default().fg(TEXT))
    };
    let search = Paragraph::new(Line::from(vec![
        Span::styled(" Search: ", Style::default().fg(SUBTEXT)),
        search_text,
        Span::styled("_", Style::default().fg(BLUE)),
    ]));
    f.render_widget(search, content_chunks[0]);

    // Image list
    let images = app.filtered_cloud_images();
    let items: Vec<ListItem> = images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let style = if i == app.cloud_search_idx {
                Style::default().fg(TEXT).bg(SURFACE0).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT)
            };
            let line = format!("  {} {} ({}, {})", img.name, img.version, img.arch, img.size);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, content_chunks[1]);
}

fn draw_node_init_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 12, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Add New Node ")
        .title_style(Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(GREEN))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let hostname_style = if app.node_init_field == 0 {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(SUBTEXT)
    };
    let mac_style = if app.node_init_field == 1 {
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(SUBTEXT)
    };

    let cursor = Span::styled("_", Style::default().fg(BLUE));

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Hostname: ", Style::default().fg(SUBTEXT)),
            Span::styled(&app.node_init_hostname, hostname_style),
            if app.node_init_field == 0 { cursor.clone() } else { Span::raw("") },
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  MAC:      ", Style::default().fg(SUBTEXT)),
            Span::styled(&app.node_init_mac, mac_style),
            if app.node_init_field == 1 { cursor } else { Span::raw("") },
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("Tab to switch fields, Enter to add", Style::default().fg(SUBTEXT))),
    ];

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

fn draw_confirm_remove_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 10, f.area());
    f.render_widget(Clear, area);

    let hostname = app.selected_node()
        .map(|n| n.hostname.as_str())
        .unwrap_or("node");

    let block = Block::default()
        .title(" Confirm Remove ")
        .title_style(Style::default().fg(RED).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(RED))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Remove ", Style::default().fg(TEXT)),
            Span::styled(hostname, Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::styled("?", Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  This will stop any running VM and", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled("  wipe all VM data from the node.", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y] ", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::styled("Confirm    ", Style::default().fg(TEXT)),
            Span::styled("[n/Esc] ", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(TEXT)),
        ]),
    ];

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

fn draw_help_overlay(f: &mut Frame) {
    let area = centered_rect(50, 18, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help ")
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(" Navigation", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  j/k or ↑/↓     ", Style::default().fg(BLUE)), Span::styled("Move up/down", Style::default().fg(TEXT))]),
        Line::from(vec![Span::styled("  Enter          ", Style::default().fg(BLUE)), Span::styled("Node actions menu", Style::default().fg(TEXT))]),
        Line::from(""),
        Line::from(Span::styled(" General", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD))),
        Line::from(vec![Span::styled("  n              ", Style::default().fg(BLUE)), Span::styled("Add new node", Style::default().fg(TEXT))]),
        Line::from(vec![Span::styled("  i              ", Style::default().fg(BLUE)), Span::styled("Download images", Style::default().fg(TEXT))]),
        Line::from(vec![Span::styled("  r              ", Style::default().fg(BLUE)), Span::styled("Refresh", Style::default().fg(TEXT))]),
        Line::from(vec![Span::styled("  ?              ", Style::default().fg(BLUE)), Span::styled("Help", Style::default().fg(TEXT))]),
        Line::from(vec![Span::styled("  q              ", Style::default().fg(BLUE)), Span::styled("Quit", Style::default().fg(TEXT))]),
    ];

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

// ============================================================================
// Action Progress Overlay
// ============================================================================

fn draw_action_progress_overlay(f: &mut Frame, message: &str) {
    // Count lines in message to size overlay appropriately
    let msg_lines: Vec<&str> = message.lines().collect();
    let height = (msg_lines.len() + 4).max(7) as u16; // +4 for padding and borders
    let area = centered_rect(60, height, f.area());

    // Clear area
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Please Wait ")
        .title_style(Style::default().fg(MAUVE).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MAUVE))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build lines from message, splitting on newlines
    let mut lines = vec![Line::from("")];
    for line in msg_lines {
        lines.push(Line::from(Span::styled(
            line,
            Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)
        )));
    }
    lines.push(Line::from(""));

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(para, inner);
}

// ============================================================================
// Server Panel
// ============================================================================

fn draw_server_panel(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Server ")
        .title_style(Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SURFACE1))
        .style(Style::default().bg(BASE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Calculate time since last refresh
    let elapsed = app.last_refresh.elapsed().as_secs();
    let refresh_text = if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else {
        format!("{}m ago", elapsed / 60)
    };

    let pxe_status = if app.pxe_running {
        Span::styled("Running", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("Stopped", Style::default().fg(RED).add_modifier(Modifier::BOLD))
    };

    // Show CLI command hint based on status
    let server_hint = if app.pxe_running {
        Line::from(vec![
            Span::styled(" sudo cave server stop", Style::default().fg(SUBTEXT)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" sudo cave server start", Style::default().fg(SUBTEXT)),
        ])
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("── PXE Server ──", Style::default().fg(MAUVE))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Status  ", Style::default().fg(SUBTEXT)),
            pxe_status,
        ]),
        Line::from(vec![
            Span::styled(" Port    ", Style::default().fg(SUBTEXT)),
            Span::styled(format!("{}", app.http_port), Style::default().fg(TEXT)),
        ]),
        server_hint,
        Line::from(""),
        Line::from(Span::styled("── Poller ──", Style::default().fg(MAUVE))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Last    ", Style::default().fg(SUBTEXT)),
            Span::styled(&refresh_text, Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(Span::styled("── Controls ──", Style::default().fg(MAUVE))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" [r]", Style::default().fg(BLUE)),
            Span::styled(" refresh", Style::default().fg(SUBTEXT)),
        ]),
    ];

    let para = Paragraph::new(lines);
    f.render_widget(para, inner);
}

// ============================================================================
// Helpers
// ============================================================================

fn format_size(bytes: u64) -> String {
    // Use decimal units (like disk manufacturers) to match CLI output
    const GB: u64 = 1_000_000_000;
    const TB: u64 = 1_000_000_000_000;

    if bytes >= TB {
        let tb = bytes as f64 / TB as f64;
        if (tb - tb.round()).abs() < 0.05 {
            format!("{} TB", tb.round() as u64)
        } else {
            format!("{:.1} TB", tb)
        }
    } else if bytes >= GB {
        format!("{} GB", bytes / GB)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

/// Format RAM usage as "X.X% (Y GB / Z GB)" (used / total)
/// Returns (formatted_string, percentage) for color coding
fn format_ram_usage(used_mb: u64, total_mb: u64) -> (String, f64) {
    // Round total to nearest physical RAM size (OS reports less than physical)
    let physical_mb = round_to_physical_ram(total_mb);
    let physical_gb = physical_mb / 1024;

    // Add BIOS-reserved RAM to usage (physical - OS visible)
    let bios_reserved = physical_mb.saturating_sub(total_mb);
    let actual_used_mb = used_mb + bios_reserved;

    let used_gb = actual_used_mb as f64 / 1024.0;
    let percent = if physical_mb > 0 {
        actual_used_mb as f64 / physical_mb as f64 * 100.0
    } else {
        0.0
    };

    (format!("{:.1}% ({:.1} GB / {} GB)", percent, used_gb, physical_gb), percent)
}

/// Get color based on usage percentage
fn usage_color(percent: f64) -> Color {
    if percent < 50.0 {
        GREEN
    } else if percent < 80.0 {
        YELLOW
    } else {
        RED
    }
}

/// Round OS-reported RAM to actual physical RAM size
fn round_to_physical_ram(reported_mb: u64) -> u64 {
    // Common physical RAM sizes in MB
    const SIZES: &[u64] = &[
        2 * 1024,   // 2 GB
        4 * 1024,   // 4 GB
        8 * 1024,   // 8 GB
        16 * 1024,  // 16 GB
        32 * 1024,  // 32 GB
        64 * 1024,  // 64 GB
        128 * 1024, // 128 GB
        256 * 1024, // 256 GB
    ];

    // Find the smallest physical size >= reported (with ~5% tolerance for OS overhead)
    for &size in SIZES {
        if reported_mb <= size && reported_mb as f64 >= size as f64 * 0.90 {
            return size;
        }
    }

    // If no match, round up to nearest GB
    ((reported_mb + 1023) / 1024) * 1024
}

/// Format VM memory usage as "X.X% (Y GB / Z GB)" (used / allocated)
/// Returns (formatted_string, Option<percentage>) for color coding
fn format_vm_memory_usage(vm: &crate::config::CachedVm) -> (String, Option<f64>) {
    // Parse allocated memory from string like "2048M" or "4096M"
    let allocated_mb: u64 = vm.memory
        .trim_end_matches('M')
        .parse()
        .unwrap_or(0);

    if let Some(used_mb) = vm.memory_used_mb {
        let used_gb = used_mb as f64 / 1024.0;
        let allocated_gb = allocated_mb as f64 / 1024.0;
        let percent = if allocated_mb > 0 {
            used_mb as f64 / allocated_mb as f64 * 100.0
        } else {
            0.0
        };

        // Format allocated as integer if it's a whole number
        let alloc_str = if allocated_gb == allocated_gb.floor() {
            format!("{}", allocated_gb as u64)
        } else {
            format!("{:.1}", allocated_gb)
        };

        (format!("{:.1}% ({:.1} GB / {} GB)", percent, used_gb, alloc_str), Some(percent))
    } else {
        // No usage data, just show allocated
        let allocated_gb = allocated_mb as f64 / 1024.0;
        let display = if allocated_gb == allocated_gb.floor() {
            format!("{} GB", allocated_gb as u64)
        } else {
            format!("{:.1} GB", allocated_gb)
        };
        (display, None)
    }
}
