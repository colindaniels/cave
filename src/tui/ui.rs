use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, BorderType, Clear, Gauge, HighlightSpacing, List, ListItem,
        Paragraph, Row, Table, Tabs, Wrap,
    },
    Frame,
};

use super::app::{App, DeployStep, Focus, Screen, MEMORY_OPTIONS, CPU_OPTIONS, DISK_OPTIONS};
use super::widgets::logo;
use crate::status::NodeStatus;

// Color scheme - Catppuccin Mocha inspired
const BG: Color = Color::Rgb(30, 30, 46);
const SURFACE: Color = Color::Rgb(49, 50, 68);
const OVERLAY: Color = Color::Rgb(69, 71, 90);
const TEXT: Color = Color::Rgb(205, 214, 244);
const SUBTEXT: Color = Color::Rgb(166, 173, 200);
const LAVENDER: Color = Color::Rgb(180, 190, 254);
const BLUE: Color = Color::Rgb(137, 180, 250);
const SAPPHIRE: Color = Color::Rgb(116, 199, 236);
const GREEN: Color = Color::Rgb(166, 227, 161);
const YELLOW: Color = Color::Rgb(249, 226, 175);
const PEACH: Color = Color::Rgb(250, 179, 135);
const RED: Color = Color::Rgb(243, 139, 168);
const MAUVE: Color = Color::Rgb(203, 166, 247);

pub fn draw(f: &mut Frame, app: &App) {
    // Clear with background
    let area = f.area();

    match app.screen {
        Screen::Dashboard => draw_dashboard(f, app, area),
        Screen::Deploy => draw_deploy(f, app, area),
        Screen::Images => draw_images(f, app, area),
        Screen::Help => draw_help(f, area),
        Screen::Logs => draw_logs(f, app, area),
    }

    // Draw popup if active
    if app.show_popup {
        draw_popup(f, &app.popup_message);
    }
}

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Logo
            Constraint::Min(10),    // Main content
            Constraint::Length(3), // Status bar
        ])
        .split(area);

    // Logo
    draw_logo(f, chunks[0]);

    // Main content - three columns
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(45), // Nodes
            Constraint::Percentage(35), // Images
            Constraint::Percentage(20), // Server status
        ])
        .split(chunks[1]);

    draw_nodes_panel(f, app, main_chunks[0]);
    draw_images_panel(f, app, main_chunks[1]);
    draw_status_panel(f, app, main_chunks[2]);

    // Status bar
    draw_status_bar(f, app, chunks[2]);
}

fn draw_logo(f: &mut Frame, area: Rect) {
    let logo_text = logo::LOGO;
    let logo = Paragraph::new(logo_text)
        .style(Style::default().fg(MAUVE))
        .alignment(Alignment::Center)
        .block(Block::default());
    f.render_widget(logo, area);
}

fn draw_nodes_panel(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Nodes;
    let border_color = if is_focused { LAVENDER } else { OVERLAY };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰒋 ", Style::default().fg(BLUE)),
            Span::styled("Nodes", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.nodes.is_empty() {
        let empty = Paragraph::new("No nodes registered")
            .style(Style::default().fg(SUBTEXT))
            .alignment(Alignment::Center);
        f.render_widget(empty, inner);
        return;
    }

    // Create table rows
    let rows: Vec<Row> = app
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(i, node)| {
            let is_selected = i == app.selected_node_idx && is_focused;
            let _status_style = match node.status {
                NodeStatus::Active => Style::default().fg(GREEN),
                NodeStatus::Standby => Style::default().fg(YELLOW),
                NodeStatus::Offline => Style::default().fg(RED).dim(),
            };
            let status_icon = match node.status {
                NodeStatus::Active => "●",
                NodeStatus::Standby => "◐",
                NodeStatus::Offline => "○",
            };

            let row_style = if is_selected {
                Style::default().bg(SURFACE).fg(TEXT)
            } else {
                Style::default().fg(TEXT)
            };

            let mut rows = vec![Row::new(vec![
                format!(" {} {}", status_icon, node.node.hostname),
                node.ip.clone(),
                format!("{}", match node.status {
                    NodeStatus::Active => "active",
                    NodeStatus::Standby => "standby",
                    NodeStatus::Offline => "offline",
                }),
            ])
            .style(row_style)
            .height(1)];

            // Add VM row if active
            if node.status == NodeStatus::Active {
                if let Some(ref vm_name) = node.vm_name {
                    let vm_ip = node.vm_ip.as_deref().unwrap_or("booting...");
                    rows.push(
                        Row::new(vec![
                            format!("   └─ {}", vm_name),
                            vm_ip.to_string(),
                            "running".to_string(),
                        ])
                        .style(Style::default().fg(SAPPHIRE))
                        .height(1),
                    );
                }
            }

            rows
        })
        .collect();

    let header = Row::new(vec!["Name", "IP", "Status"])
        .style(Style::default().fg(SUBTEXT).add_modifier(Modifier::BOLD))
        .height(1);

    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(35),
        Constraint::Percentage(25),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(SURFACE));

    f.render_widget(table, inner);
}

fn draw_images_panel(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Images;
    let border_color = if is_focused { LAVENDER } else { OVERLAY };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰋊 ", Style::default().fg(PEACH)),
            Span::styled("Images", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.images.is_empty() {
        let empty = Paragraph::new("No images downloaded")
            .style(Style::default().fg(SUBTEXT))
            .alignment(Alignment::Center);
        f.render_widget(empty, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let is_selected = i == app.selected_image_idx && is_focused;
            let style = if is_selected {
                Style::default().bg(SURFACE).fg(TEXT)
            } else {
                Style::default().fg(TEXT)
            };
            let size = format_size(img.size);
            ListItem::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(&img.name, style),
                Span::raw(" "),
                Span::styled(size, Style::default().fg(SUBTEXT)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().bg(SURFACE))
        .highlight_spacing(HighlightSpacing::Always);

    f.render_widget(list, inner);
}

fn draw_status_panel(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰒍 ", Style::default().fg(SAPPHIRE)),
            Span::styled("Server", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let status_text = if app.server_running {
        Line::from(vec![
            Span::styled("● ", Style::default().fg(GREEN)),
            Span::styled("Running", Style::default().fg(GREEN)),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ ", Style::default().fg(RED)),
            Span::styled("Stopped", Style::default().fg(RED)),
        ])
    };

    let active_count = app.nodes.iter().filter(|n| n.status == NodeStatus::Active).count();
    let standby_count = app.nodes.iter().filter(|n| n.status == NodeStatus::Standby).count();
    let offline_count = app.nodes.iter().filter(|n| n.status == NodeStatus::Offline).count();

    let refresh_line = if app.refreshing {
        Line::from(vec![
            Span::styled(" ◐ ", Style::default().fg(YELLOW)),
            Span::styled("Refreshing...", Style::default().fg(YELLOW)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(GREEN)),
            Span::styled("Ready", Style::default().fg(SUBTEXT)),
        ])
    };

    let stats = vec![
        Line::from(""),
        refresh_line,
        Line::from(""),
        Line::from(vec![
            Span::styled(" PXE Server", Style::default().fg(SUBTEXT)),
        ]),
        status_text,
        Line::from(""),
        Line::from(vec![
            Span::styled(" Nodes", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {} ", active_count), Style::default().fg(GREEN)),
            Span::styled("active", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {} ", standby_count), Style::default().fg(YELLOW)),
            Span::styled("standby", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {} ", offline_count), Style::default().fg(RED)),
            Span::styled("offline", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Images", Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {}", app.images.len()), Style::default().fg(TEXT)),
        ]),
    ];

    let para = Paragraph::new(stats);
    f.render_widget(para, inner);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.screen {
        Screen::Dashboard => vec![
            ("q", "quit"),
            ("d", "deploy"),
            ("r", "refresh"),
            ("Tab", "focus"),
            ("↑↓", "select"),
            ("?", "help"),
        ],
        Screen::Deploy => vec![
            ("Esc", "cancel"),
            ("Enter", "confirm"),
            ("↑↓", "select"),
            ("Backspace", "back"),
        ],
        _ => vec![("Esc", "back"), ("q", "quit")],
    };

    let spans: Vec<Span> = hints
        .iter()
        .flat_map(|(key, desc)| {
            vec![
                Span::styled(format!(" {} ", key), Style::default().fg(BG).bg(LAVENDER)),
                Span::styled(format!(" {} ", desc), Style::default().fg(SUBTEXT)),
                Span::raw("  "),
            ]
        })
        .collect();

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(SURFACE));

    f.render_widget(bar, area);
}

fn draw_deploy(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Progress
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Status bar
        ])
        .split(area);

    // Title
    let step_num = match app.deploy_step {
        DeployStep::SelectNode => 1,
        DeployStep::SelectImage => 2,
        DeployStep::ConfigureVm => 3,
        DeployStep::Confirm => 4,
        DeployStep::Deploying => 5,
        DeployStep::Complete => 5,
    };
    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰄠 ", Style::default().fg(MAUVE)),
        Span::styled("Deploy VM", Style::default().fg(TEXT).bold()),
        Span::styled(format!("  Step {} of 5", step_num), Style::default().fg(SUBTEXT)),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    // Progress tabs
    let steps = vec!["Node", "Image", "Config", "Confirm", "Deploy"];
    let tabs = Tabs::new(steps)
        .select(step_num - 1)
        .style(Style::default().fg(SUBTEXT))
        .highlight_style(Style::default().fg(LAVENDER).bold())
        .divider("→");
    f.render_widget(tabs, chunks[1]);

    // Content based on step
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = content_block.inner(chunks[2]);
    f.render_widget(content_block, chunks[2]);

    match app.deploy_step {
        DeployStep::SelectNode => draw_deploy_node_select(f, app, inner),
        DeployStep::SelectImage => draw_deploy_image_select(f, app, inner),
        DeployStep::ConfigureVm => draw_deploy_config(f, app, inner),
        DeployStep::Confirm => draw_deploy_confirm(f, app, inner),
        DeployStep::Deploying => draw_deploy_progress(f, app, inner),
        DeployStep::Complete => draw_deploy_complete(f, app, inner),
    }

    // Status bar
    draw_status_bar(f, app, chunks[3]);
}

fn draw_deploy_node_select(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let is_selected = i == app.deploy_node_idx;
            let style = if is_selected {
                Style::default().bg(LAVENDER).fg(BG)
            } else {
                Style::default().fg(TEXT)
            };
            let status_color = match node.status {
                NodeStatus::Active => GREEN,
                NodeStatus::Standby => YELLOW,
                NodeStatus::Offline => RED,
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_selected { " ▸ " } else { "   " }, style),
                Span::styled(&node.node.hostname, style.bold()),
                Span::styled(format!("  {}", node.ip), style),
                Span::styled("  ●", Style::default().fg(status_color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(" Select Node ").title_style(Style::default().fg(SUBTEXT)));

    f.render_widget(list, area);
}

fn draw_deploy_image_select(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let is_selected = i == app.deploy_image_idx;
            let style = if is_selected {
                Style::default().bg(LAVENDER).fg(BG)
            } else {
                Style::default().fg(TEXT)
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_selected { " ▸ " } else { "   " }, style),
                Span::styled(&img.name, style),
                Span::styled(format!("  {}", format_size(img.size)), Style::default().fg(SUBTEXT)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(" Select Image ").title_style(Style::default().fg(SUBTEXT)));

    f.render_widget(list, area);
}

fn draw_deploy_config(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    // VM Name
    let name_block = Block::default()
        .title(" VM Name ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(SUBTEXT));
    let name = Paragraph::new(format!(" {}", app.deploy_vm_name))
        .style(Style::default().fg(TEXT))
        .block(name_block);
    f.render_widget(name, chunks[0]);

    // Memory selection
    let mem_items: Vec<Span> = MEMORY_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            if i == app.deploy_memory_idx {
                Span::styled(format!(" [{}] ", label), Style::default().fg(LAVENDER).bold())
            } else {
                Span::styled(format!("  {}  ", label), Style::default().fg(SUBTEXT))
            }
        })
        .collect();

    let mem = Paragraph::new(Line::from(mem_items))
        .block(
            Block::default()
                .title(" Memory (←/→) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SUBTEXT)),
        );
    f.render_widget(mem, chunks[1]);

    // CPU selection
    let cpu_items: Vec<Span> = CPU_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            if i == app.deploy_cpu_idx {
                Span::styled(format!(" [{}] ", label), Style::default().fg(LAVENDER).bold())
            } else {
                Span::styled(format!("  {}  ", label), Style::default().fg(SUBTEXT))
            }
        })
        .collect();

    let cpu = Paragraph::new(Line::from(cpu_items))
        .block(
            Block::default()
                .title(" CPUs (Tab) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SUBTEXT)),
        );
    f.render_widget(cpu, chunks[2]);

    // Disk size selection
    let disk_items: Vec<Span> = DISK_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            if i == app.deploy_disk_idx {
                Span::styled(format!(" [{}] ", label), Style::default().fg(LAVENDER).bold())
            } else {
                Span::styled(format!("  {}  ", label), Style::default().fg(SUBTEXT))
            }
        })
        .collect();

    let disk = Paragraph::new(Line::from(disk_items))
        .block(
            Block::default()
                .title(" Disk ([/]) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SUBTEXT)),
        );
    f.render_widget(disk, chunks[3]);
}

fn draw_deploy_confirm(f: &mut Frame, app: &App, area: Rect) {
    let node = &app.nodes[app.deploy_node_idx];
    let image = &app.images[app.deploy_image_idx];
    let memory = MEMORY_OPTIONS[app.deploy_memory_idx].1;
    let cpus = CPU_OPTIONS[app.deploy_cpu_idx].1;
    let disk = DISK_OPTIONS[app.deploy_disk_idx].1;

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Ready to deploy:", Style::default().fg(TEXT).bold()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Node:   ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.node.hostname, Style::default().fg(SAPPHIRE)),
            Span::styled(format!(" ({})", node.ip), Style::default().fg(SUBTEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Image:  ", Style::default().fg(SUBTEXT)),
            Span::styled(&image.name, Style::default().fg(PEACH)),
        ]),
        Line::from(vec![
            Span::styled("  VM:     ", Style::default().fg(SUBTEXT)),
            Span::styled(&app.deploy_vm_name, Style::default().fg(GREEN)),
        ]),
        Line::from(vec![
            Span::styled("  Memory: ", Style::default().fg(SUBTEXT)),
            Span::styled(memory, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  CPUs:   ", Style::default().fg(SUBTEXT)),
            Span::styled(cpus, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Disk:   ", Style::default().fg(SUBTEXT)),
            Span::styled(disk, Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(SUBTEXT)),
            Span::styled("Enter", Style::default().fg(LAVENDER).bold()),
            Span::styled(" to deploy or ", Style::default().fg(SUBTEXT)),
            Span::styled("Esc", Style::default().fg(LAVENDER).bold()),
            Span::styled(" to cancel", Style::default().fg(SUBTEXT)),
        ]),
    ];

    let para = Paragraph::new(text);
    f.render_widget(para, area);
}

fn draw_deploy_progress(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let gauge = Gauge::default()
        .block(Block::default().title(" Progress ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(LAVENDER).bg(SURFACE))
        .percent((app.deploy_progress * 100.0) as u16)
        .label(format!("{}%", (app.deploy_progress * 100.0) as u16));

    f.render_widget(gauge, chunks[0]);

    let status = Paragraph::new(format!("  {}", app.deploy_status))
        .style(Style::default().fg(SUBTEXT));
    f.render_widget(status, chunks[1]);

    let hint = Paragraph::new("  Press Enter to simulate progress...")
        .style(Style::default().fg(SUBTEXT).dim());
    f.render_widget(hint, chunks[2]);
}

fn draw_deploy_complete(f: &mut Frame, app: &App, area: Rect) {
    let node = &app.nodes[app.deploy_node_idx];

    let text = vec![
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(GREEN).bold()),
            Span::styled("Deployment Complete!", Style::default().fg(GREEN).bold()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  VM '{}' is now running on {}", app.deploy_vm_name, node.node.hostname),
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Alpine host: ", Style::default().fg(SUBTEXT)),
            Span::styled(format!("ssh root@{}", node.ip), Style::default().fg(SAPPHIRE)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(SUBTEXT)),
            Span::styled("Enter", Style::default().fg(LAVENDER).bold()),
            Span::styled(" to return to dashboard", Style::default().fg(SUBTEXT)),
        ]),
    ];

    let para = Paragraph::new(text);
    f.render_widget(para, area);
}

fn draw_images(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰋊 ", Style::default().fg(PEACH)),
        Span::styled("Images", Style::default().fg(TEXT).bold()),
        Span::styled(format!("  {} total", app.images.len()), Style::default().fg(SUBTEXT)),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(chunks[1]);
    f.render_widget(block, chunks[1]);

    if app.images.is_empty() {
        let empty = Paragraph::new("\n  No images found\n\n  Download with: cave image pull <url>")
            .style(Style::default().fg(SUBTEXT));
        f.render_widget(empty, inner);
    } else {
        let rows: Vec<Row> = app
            .images
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let style = if i == app.selected_image_idx {
                    Style::default().bg(SURFACE).fg(TEXT)
                } else {
                    Style::default().fg(TEXT)
                };
                Row::new(vec![
                    format!(" {}", img.name),
                    format_size(img.size),
                ])
                .style(style)
            })
            .collect();

        let header = Row::new(vec!["Name", "Size"])
            .style(Style::default().fg(SUBTEXT).bold());

        let widths = [Constraint::Percentage(70), Constraint::Percentage(30)];
        let table = Table::new(rows, widths).header(header);
        f.render_widget(table, inner);
    }

    draw_status_bar(f, app, chunks[2]);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled("  Keyboard Shortcuts", Style::default().fg(LAVENDER).bold())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  q        ", Style::default().fg(PEACH)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("  d        ", Style::default().fg(PEACH)),
            Span::raw("Deploy VM"),
        ]),
        Line::from(vec![
            Span::styled("  r        ", Style::default().fg(PEACH)),
            Span::raw("Refresh node status"),
        ]),
        Line::from(vec![
            Span::styled("  i        ", Style::default().fg(PEACH)),
            Span::raw("View images"),
        ]),
        Line::from(vec![
            Span::styled("  l        ", Style::default().fg(PEACH)),
            Span::raw("View logs"),
        ]),
        Line::from(vec![
            Span::styled("  Tab      ", Style::default().fg(PEACH)),
            Span::raw("Switch focus"),
        ]),
        Line::from(vec![
            Span::styled("  ↑/↓ j/k  ", Style::default().fg(PEACH)),
            Span::raw("Navigate"),
        ]),
        Line::from(vec![
            Span::styled("  Enter    ", Style::default().fg(PEACH)),
            Span::raw("Select/Confirm"),
        ]),
        Line::from(vec![
            Span::styled("  Esc      ", Style::default().fg(PEACH)),
            Span::raw("Back/Cancel"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(SUBTEXT)),
            Span::styled("Esc", Style::default().fg(LAVENDER)),
            Span::styled(" to close", Style::default().fg(SUBTEXT)),
        ]),
    ];

    let para = Paragraph::new(help_text).style(Style::default().fg(TEXT));
    f.render_widget(para, inner);
}

fn draw_logs(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰍡 ", Style::default().fg(SAPPHIRE)),
        Span::styled("Logs", Style::default().fg(TEXT).bold()),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(chunks[1]);
    f.render_widget(block, chunks[1]);

    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .rev()
        .take(inner.height as usize)
        .rev()
        .map(|l| Line::from(format!("  {}", l)))
        .collect();

    let para = Paragraph::new(log_lines).style(Style::default().fg(SUBTEXT));
    f.render_widget(para, inner);

    draw_status_bar(f, app, chunks[2]);
}

fn draw_popup(f: &mut Frame, message: &str) {
    let area = centered_rect(50, 20, f.area());

    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Notice ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(YELLOW))
        .style(Style::default().bg(SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = Paragraph::new(format!("\n  {}\n\n  Press Enter to close", message))
        .style(Style::default().fg(TEXT))
        .wrap(Wrap { trim: true });

    f.render_widget(text, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    }
}
