use console::{style, Term};

// Styled output helpers
pub fn print_header(text: &str) {
    println!("\n{}", style(text).bold().underlined());
}

pub fn print_success(text: &str) {
    println!("{} {}", style("✓").green().bold(), text);
}

pub fn print_error(text: &str) {
    println!("{} {}", style("✗").red().bold(), text);
}

pub fn print_warning(text: &str) {
    println!("{} {}", style("!").yellow().bold(), text);
}

pub fn print_info(text: &str) {
    println!("{} {}", style("•").cyan(), text);
}

// Box drawing for sections
pub fn print_box(title: &str, lines: &[(&str, &str)]) {
    let term_width = Term::stdout().size().1 as usize;
    let box_width = term_width.min(60);

    // Top border with title
    let title_display = format!(" {} ", title);
    let padding = box_width.saturating_sub(title_display.len() + 2);
    let left_pad = padding / 2;
    let right_pad = padding - left_pad;

    println!(
        "{}{}{}",
        style("─".repeat(left_pad)).dim(),
        style(&title_display).bold(),
        style("─".repeat(right_pad)).dim()
    );

    // Content
    for (label, value) in lines {
        println!("  {} {}", style(format!("{}:", label)).dim(), value);
    }

    // Bottom border
    println!("{}", style("─".repeat(box_width)).dim());
}

// Completion banner
pub fn print_completion(title: &str) {
    println!();
    println!("{}", style("─".repeat(40)).dim());
    println!(
        "{} {}",
        style("✓").green().bold(),
        style(title).green().bold()
    );
    println!("{}", style("─".repeat(40)).dim());
}

// Format helpers
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_memory(mb: u32) -> String {
    if mb >= 1024 {
        format!("{:.1} GB", mb as f64 / 1024.0)
    } else {
        format!("{} MB", mb)
    }
}
