use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, BorderType, List, ListItem, Paragraph, Row, Table,
    },
    Frame,
};

use super::app::{App, Screen, NODE_ACTIONS};
use super::widgets::logo;
use crate::commands::images::CLOUD_IMAGES;

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
    match app.screen {
        Screen::Dashboard => draw_dashboard(f, app, f.area()),
        Screen::NodeDetails => draw_node_details(f, app, f.area()),
        Screen::Images => draw_images(f, app, f.area()),
        Screen::ImageDownload => draw_image_download(f, app, f.area()),
        Screen::Help => draw_help(f, f.area()),
    }

    // Draw status message if present
    if let Some((ref msg, _)) = app.status_message {
        draw_status_toast(f, msg);
    }
}

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Logo
            Constraint::Min(10),    // Main content
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    // Logo
    draw_logo(f, chunks[0]);

    // Main content - two columns
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(65), // Nodes
            Constraint::Percentage(35), // Status panel
        ])
        .split(chunks[1]);

    draw_nodes_panel(f, app, main_chunks[0]);
    draw_status_panel(f, app, main_chunks[1]);

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
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰒋 ", Style::default().fg(BLUE)),
            Span::styled("Nodes", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.nodes.is_empty() {
        let empty = Paragraph::new("\n  No nodes registered\n\n  Run: cave node init <hostname> <mac>")
            .style(Style::default().fg(SUBTEXT));
        f.render_widget(empty, inner);
        return;
    }

    // Create table rows
    let rows: Vec<Row> = app
        .nodes
        .iter()
        .enumerate()
        .flat_map(|(i, node)| {
            let is_selected = i == app.selected_node_idx;

            let (_status_color, status_icon) = match node.status.as_str() {
                "active" => (GREEN, "●"),
                "standby" => (YELLOW, "◐"),
                _ => (RED, "○"),
            };

            let row_style = if is_selected {
                Style::default().bg(SURFACE).fg(TEXT)
            } else {
                Style::default().fg(TEXT)
            };

            // Format specs compactly
            let specs = if node.status != "offline" {
                format!("{} · {}c/{}",
                    truncate(&node.cpu, 16),
                    node.cores.replace(" cores", ""),
                    &node.ram
                )
            } else {
                "─".to_string()
            };

            let ip_display = if node.status == "offline" {
                "─".to_string()
            } else {
                node.ip.clone().unwrap_or_else(|| "─".to_string())
            };

            let mut rows = vec![Row::new(vec![
                format!(" {} {}", status_icon, node.hostname),
                ip_display,
                node.status.clone(),
                specs,
            ])
            .style(row_style)
            .height(1)];

            // Add VM row if present
            if let Some(ref vm) = node.vm {
                let vm_ip = if vm.ip.is_empty() {
                    "booting...".to_string()
                } else {
                    vm.ip.clone()
                };
                rows.push(
                    Row::new(vec![
                        format!("   └─ {}", vm.name),
                        vm_ip,
                        "running".to_string(),
                        format!("{}, {} CPU", vm.memory, vm.cpus),
                    ])
                    .style(Style::default().fg(SAPPHIRE))
                    .height(1),
                );
            }

            rows
        })
        .collect();

    let header = Row::new(vec!["Name", "IP", "Status", "Specs"])
        .style(Style::default().fg(SUBTEXT).add_modifier(Modifier::BOLD))
        .height(1);

    let widths = [
        Constraint::Percentage(25),
        Constraint::Percentage(20),
        Constraint::Percentage(15),
        Constraint::Percentage(40),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(SURFACE));

    f.render_widget(table, inner);
}

fn draw_status_panel(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰒍 ", Style::default().fg(SAPPHIRE)),
            Span::styled("Status", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let server_status = if app.server_running {
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

    let active_count = app.nodes.iter().filter(|n| n.status == "active").count();
    let standby_count = app.nodes.iter().filter(|n| n.status == "standby").count();
    let offline_count = app.nodes.iter().filter(|n| n.status == "offline").count();

    let stats = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" PXE Server", Style::default().fg(SUBTEXT)),
        ]),
        server_status,
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

fn draw_node_details(f: &mut Frame, app: &App, area: Rect) {
    let node = match app.selected_node() {
        Some(n) => n,
        None => return,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),    // Content
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    // Title bar
    let (status_color, status_icon) = match node.status.as_str() {
        "active" => (GREEN, "●"),
        "standby" => (YELLOW, "◐"),
        _ => (RED, "○"),
    };

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰒋 ", Style::default().fg(BLUE)),
        Span::styled(&node.hostname, Style::default().fg(TEXT).bold()),
        Span::raw("  "),
        Span::styled(status_icon, Style::default().fg(status_color)),
        Span::styled(format!(" {}", node.status), Style::default().fg(status_color)),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    // Main content - two columns
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Info
            Constraint::Percentage(50), // Actions
        ])
        .split(chunks[1]);

    draw_node_info(f, app, main_chunks[0]);
    draw_node_actions(f, app, main_chunks[1]);

    // Status bar
    draw_status_bar(f, app, chunks[2]);
}

fn draw_node_info(f: &mut Frame, app: &App, area: Rect) {
    let node = match app.selected_node() {
        Some(n) => n,
        None => return,
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰋊 ", Style::default().fg(PEACH)),
            Span::styled("Specs", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let ip_display = node.ip.clone().unwrap_or_else(|| "─".to_string());

    // Format disk info
    let disk_info = if node.disks.is_empty() {
        "No disks".to_string()
    } else {
        node.disks.iter().map(|d| {
            let size = format_bytes(d.size_bytes);
            format!("{} {}", size, d.disk_type)
        }).collect::<Vec<_>>().join(" + ")
    };

    let mut info_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Hostname: ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.hostname, Style::default().fg(TEXT).bold()),
        ]),
        Line::from(vec![
            Span::styled("  MAC:      ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.mac, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  IP:       ", Style::default().fg(SUBTEXT)),
            Span::styled(&ip_display, Style::default().fg(SAPPHIRE)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  CPU:      ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.cpu, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Cores:    ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.cores, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  RAM:      ", Style::default().fg(SUBTEXT)),
            Span::styled(&node.ram, Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Disks:    ", Style::default().fg(SUBTEXT)),
            Span::styled(&disk_info, Style::default().fg(TEXT)),
        ]),
    ];

    // Add VM info if present
    if let Some(ref vm) = node.vm {
        info_lines.push(Line::from(""));
        info_lines.push(Line::from(vec![
            Span::styled("  VM", Style::default().fg(SAPPHIRE).bold()),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  Name:     ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.name, Style::default().fg(SAPPHIRE)),
        ]));
        let vm_ip_display = if vm.ip.is_empty() { "booting..." } else { &vm.ip };
        info_lines.push(Line::from(vec![
            Span::styled("  IP:       ", Style::default().fg(SUBTEXT)),
            Span::styled(vm_ip_display, Style::default().fg(SAPPHIRE)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  Memory:   ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.memory, Style::default().fg(TEXT)),
        ]));
        info_lines.push(Line::from(vec![
            Span::styled("  CPUs:     ", Style::default().fg(SUBTEXT)),
            Span::styled(&vm.cpus, Style::default().fg(TEXT)),
        ]));
    }

    let para = Paragraph::new(info_lines);
    f.render_widget(para, inner);
}

fn draw_node_actions(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰌑 ", Style::default().fg(LAVENDER)),
            Span::styled("Actions", Style::default().fg(TEXT).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = NODE_ACTIONS
        .iter()
        .enumerate()
        .map(|(i, (_, name, desc))| {
            let is_selected = i == app.selected_action_idx;
            let style = if is_selected {
                Style::default().bg(LAVENDER).fg(BG)
            } else {
                Style::default().fg(TEXT)
            };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(if is_selected { " ▸ " } else { "   " }, style),
                    Span::styled(*name, style.bold()),
                ]),
                Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled(*desc, Style::default().fg(SUBTEXT)),
                ]),
            ])
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_images(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(3),  // Search bar
            Constraint::Min(10),    // Image list
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    let filtered = app.filtered_images();

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰋊 ", Style::default().fg(PEACH)),
        Span::styled("Images", Style::default().fg(TEXT).bold()),
        Span::styled(
            format!("  {} of {} shown", filtered.len(), app.images.len()),
            Style::default().fg(SUBTEXT),
        ),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    // Search bar
    let search_border_color = if app.image_search_active { LAVENDER } else { OVERLAY };
    let search_block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰍉 ", Style::default().fg(SAPPHIRE)),
            Span::styled("Search", Style::default().fg(SUBTEXT)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(search_border_color))
        .style(Style::default().bg(BG));

    let search_inner = search_block.inner(chunks[1]);
    f.render_widget(search_block, chunks[1]);

    let cursor = if app.image_search_active { "▌" } else { "" };
    let search_text = if app.image_search.is_empty() && !app.image_search_active {
        Paragraph::new(Line::from(vec![
            Span::styled(" Type to search or press ", Style::default().fg(SUBTEXT).dim()),
            Span::styled("/", Style::default().fg(LAVENDER)),
            Span::styled(" to focus", Style::default().fg(SUBTEXT).dim()),
        ]))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {}", app.image_search), Style::default().fg(TEXT)),
            Span::styled(cursor, Style::default().fg(LAVENDER)),
        ]))
    };
    f.render_widget(search_text, search_inner);

    // Image list
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(chunks[2]);
    f.render_widget(block, chunks[2]);

    if filtered.is_empty() {
        let msg = if app.image_search.is_empty() {
            "\n  No images found\n\n  Download with: cave image pull <url>"
        } else {
            "\n  No images match your search"
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(SUBTEXT));
        f.render_widget(empty, inner);
    } else {
        let rows: Vec<Row> = filtered
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let is_selected = i == app.selected_image_idx;
                let style = if is_selected {
                    Style::default().bg(SURFACE).fg(TEXT)
                } else {
                    Style::default().fg(TEXT)
                };
                Row::new(vec![
                    format!(" {}", img.display_name),
                    format_size(img.size),
                    img.filename.clone(),
                ])
                .style(style)
            })
            .collect();

        let header = Row::new(vec!["Name", "Size", "Filename"])
            .style(Style::default().fg(SUBTEXT).bold());

        let widths = [
            Constraint::Percentage(40),
            Constraint::Percentage(15),
            Constraint::Percentage(45),
        ];
        let table = Table::new(rows, widths).header(header);
        f.render_widget(table, inner);
    }

    draw_status_bar(f, app, chunks[3]);
}

fn draw_image_download(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(3),  // Search bar
            Constraint::Min(10),    // Results
            Constraint::Length(3),  // Status bar
        ])
        .split(area);

    let filtered = app.filtered_cloud_images();

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" 󰇚 ", Style::default().fg(GREEN)),
        Span::styled("Download Image", Style::default().fg(TEXT).bold()),
        Span::styled(
            format!("  {} available", CLOUD_IMAGES.len()),
            Style::default().fg(SUBTEXT),
        ),
    ]))
    .style(Style::default().bg(SURFACE));
    f.render_widget(title, chunks[0]);

    // Search bar
    let search_block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("󰍉 ", Style::default().fg(SAPPHIRE)),
            Span::styled("Search distros", Style::default().fg(SUBTEXT)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LAVENDER))
        .style(Style::default().bg(BG));

    let search_inner = search_block.inner(chunks[1]);
    f.render_widget(search_block, chunks[1]);

    let search_text = if app.cloud_search.is_empty() {
        Paragraph::new(Line::from(vec![
            Span::styled(" Type to search (ubuntu, debian, arch, alpine...)", Style::default().fg(SUBTEXT).dim()),
            Span::styled("▌", Style::default().fg(LAVENDER)),
        ]))
    } else {
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {}", app.cloud_search), Style::default().fg(TEXT)),
            Span::styled("▌", Style::default().fg(LAVENDER)),
        ]))
    };
    f.render_widget(search_text, search_inner);

    // Results list
    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(format!("{} results", filtered.len()), Style::default().fg(SUBTEXT)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OVERLAY))
        .style(Style::default().bg(BG));

    let inner = block.inner(chunks[2]);
    f.render_widget(block, chunks[2]);

    if filtered.is_empty() {
        let empty = Paragraph::new("\n  No images match your search\n\n  Try: ubuntu, debian, fedora, arch, alpine")
            .style(Style::default().fg(SUBTEXT));
        f.render_widget(empty, inner);
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let is_selected = i == app.cloud_search_idx;
                let style = if is_selected {
                    Style::default().bg(LAVENDER).fg(BG)
                } else {
                    Style::default().fg(TEXT)
                };

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(if is_selected { " ▸ " } else { "   " }, style),
                        Span::styled(format!("{} {}", img.name, img.version), style.bold()),
                    ]),
                    Line::from(vec![
                        Span::styled("     ", Style::default()),
                        Span::styled(format!("{} · {} · {}", img.arch, img.format, img.size), Style::default().fg(SUBTEXT)),
                    ]),
                ])
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, inner);
    }

    draw_status_bar(f, app, chunks[3]);
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
        Line::from(vec![Span::styled("  Dashboard", Style::default().fg(LAVENDER).bold())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  q        ", Style::default().fg(PEACH)),
            Span::raw("Quit"),
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
            Span::styled("  ?        ", Style::default().fg(PEACH)),
            Span::raw("Toggle help"),
        ]),
        Line::from(vec![
            Span::styled("  ↑/↓ j/k  ", Style::default().fg(PEACH)),
            Span::raw("Select node"),
        ]),
        Line::from(vec![
            Span::styled("  Enter    ", Style::default().fg(PEACH)),
            Span::raw("View node details"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled("  Node Details", Style::default().fg(LAVENDER).bold())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  w        ", Style::default().fg(PEACH)),
            Span::raw("Wake node (WoL)"),
        ]),
        Line::from(vec![
            Span::styled("  s        ", Style::default().fg(PEACH)),
            Span::raw("Shutdown node"),
        ]),
        Line::from(vec![
            Span::styled("  d        ", Style::default().fg(PEACH)),
            Span::raw("Deploy VM"),
        ]),
        Line::from(vec![
            Span::styled("  Esc      ", Style::default().fg(PEACH)),
            Span::raw("Back to dashboard"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled("  Images", Style::default().fg(LAVENDER).bold())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  d        ", Style::default().fg(PEACH)),
            Span::raw("Download new image"),
        ]),
        Line::from(vec![
            Span::styled("  /        ", Style::default().fg(PEACH)),
            Span::raw("Focus filter bar"),
        ]),
        Line::from(vec![
            Span::styled("  a-z      ", Style::default().fg(PEACH)),
            Span::raw("Quick filter (just start typing)"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled("  Download", Style::default().fg(LAVENDER).bold())]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  a-z      ", Style::default().fg(PEACH)),
            Span::raw("Search distros (ubuntu, arch...)"),
        ]),
        Line::from(vec![
            Span::styled("  Enter    ", Style::default().fg(PEACH)),
            Span::raw("Download selected image"),
        ]),
        Line::from(vec![
            Span::styled("  ⌫        ", Style::default().fg(PEACH)),
            Span::raw("Clear search"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(SUBTEXT)),
            Span::styled("Esc", Style::default().fg(LAVENDER)),
            Span::styled(" or ", Style::default().fg(SUBTEXT)),
            Span::styled("?", Style::default().fg(LAVENDER)),
            Span::styled(" to close", Style::default().fg(SUBTEXT)),
        ]),
    ];

    let para = Paragraph::new(help_text).style(Style::default().fg(TEXT));
    f.render_widget(para, inner);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.screen {
        Screen::Dashboard => vec![
            ("q", "quit"),
            ("r", "refresh"),
            ("i", "images"),
            ("↑↓", "select"),
            ("Enter", "details"),
            ("?", "help"),
        ],
        Screen::NodeDetails => vec![
            ("Esc", "back"),
            ("w", "wake"),
            ("s", "shutdown"),
            ("d", "deploy"),
            ("↑↓", "select"),
            ("Enter", "execute"),
        ],
        Screen::Images => vec![
            ("Esc", "back"),
            ("d", "download"),
            ("/", "filter"),
            ("↑↓", "select"),
        ],
        Screen::ImageDownload => vec![
            ("Esc", "back"),
            ("↑↓", "navigate"),
            ("Enter", "download"),
        ],
        Screen::Help => vec![
            ("Esc", "close"),
            ("?", "close"),
        ],
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

fn draw_status_toast(f: &mut Frame, message: &str) {
    let area = f.area();
    let toast_width = message.len() as u16 + 4;
    let toast_area = Rect {
        x: area.width.saturating_sub(toast_width + 2),
        y: 1,
        width: toast_width,
        height: 1,
    };

    let toast = Paragraph::new(format!(" {} ", message))
        .style(Style::default().fg(BG).bg(YELLOW));
    f.render_widget(toast, toast_area);
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
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

fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const TB: u64 = 1_000_000_000_000;

    if bytes >= TB {
        format!("{:.1}T", bytes as f64 / TB as f64)
    } else {
        format!("{}G", bytes / GB)
    }
}
