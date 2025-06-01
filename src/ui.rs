use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Code {
        input: String,
    },
    CodeOutput {
        stdout: String,
        stderr: String,
        return_code: i32,
        files: Vec<(String, String)>, // (file_id, filename)
    },
    CodeError(String),
}

pub struct App {
    pub input: String,
    pub messages: Vec<(String, Vec<MessageContent>)>, // (role, content parts)
    pub is_waiting: bool,
    pub streaming_content: Vec<MessageContent>, // Content being streamed
    pub scroll_position: usize,                 // Current scroll position
    pub auto_scroll: bool,                      // Whether to auto-scroll to bottom
    pub total_lines: usize,                     // Total number of lines in the conversation
    pub selection_mode: bool,                   // Toggle for text selection mode
    pub container_info: Option<(String, String)>, // Container ID and expiration
    pub code_execution_enabled: bool,           // Whether code execution is enabled
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: String::new(),
            messages: Vec::new(),
            is_waiting: false,
            streaming_content: Vec::new(),
            scroll_position: 0,
            auto_scroll: true,
            total_lines: 0,
            selection_mode: false,
            container_info: None,
            code_execution_enabled: false,
        }
    }
}

impl App {
    pub fn add_message(&mut self, role: String, content: String) {
        self.messages
            .push((role, vec![MessageContent::Text(content)]));
        // Auto-scroll will be handled during rendering
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
    }

    pub fn start_streaming(&mut self) {
        self.streaming_content.clear();
        // Auto-scroll will be handled during rendering
    }

    pub fn append_streaming_text(&mut self, text: &str) {
        // Find the last Text content or create a new one
        if let Some(MessageContent::Text(existing)) = self.streaming_content.last_mut() {
            existing.push_str(text);
        } else {
            self.streaming_content
                .push(MessageContent::Text(text.to_string()));
        }
    }

    pub fn add_streaming_code(&mut self, code: String) {
        self.streaming_content
            .push(MessageContent::Code { input: code });
    }

    pub fn add_streaming_output(
        &mut self,
        stdout: String,
        stderr: String,
        return_code: i32,
        files: Vec<(String, String)>,
    ) {
        self.streaming_content.push(MessageContent::CodeOutput {
            stdout,
            stderr,
            return_code,
            files,
        });
    }

    pub fn add_streaming_error(&mut self, error: String) {
        self.streaming_content
            .push(MessageContent::CodeError(error));
    }

    pub fn set_container_info(&mut self, id: String, expires_at: String) {
        self.container_info = Some((id, expires_at));
    }

    pub fn finish_streaming(&mut self) {
        if !self.streaming_content.is_empty() {
            let content = std::mem::take(&mut self.streaming_content);
            self.messages.push(("assistant".to_string(), content));
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_position = self.scroll_position.saturating_sub(amount);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_position = self.scroll_position.saturating_add(amount);
        // Don't auto-scroll unless we're at the very bottom
        self.auto_scroll = false;
    }

    pub fn update_scroll_bounds(&mut self, total_lines: usize, visible_lines: usize) {
        self.total_lines = total_lines;
        let max_scroll = total_lines.saturating_sub(visible_lines);

        // Auto-scroll to bottom if enabled
        if self.auto_scroll {
            self.scroll_position = max_scroll;
        }

        // Clamp scroll position to valid range
        self.scroll_position = self.scroll_position.min(max_scroll);

        // Re-enable auto-scroll if we're at the bottom
        if self.scroll_position == max_scroll {
            self.auto_scroll = true;
        }
    }

    pub fn toggle_selection_mode(&mut self) {
        self.selection_mode = !self.selection_mode;
    }

    pub fn toggle_code_execution(&mut self) {
        self.code_execution_enabled = !self.code_execution_enabled;
    }

    pub fn update_file_metadata(&mut self, file_id: String, filename: String) {
        // Update file metadata in all messages
        for (_, contents) in &mut self.messages {
            for content in contents {
                if let MessageContent::CodeOutput { files, .. } = content {
                    for (id, name) in files {
                        if id == &file_id {
                            *name = filename.clone();
                        }
                    }
                }
            }
        }

        // Also update in streaming content
        for content in &mut self.streaming_content {
            if let MessageContent::CodeOutput { files, .. } = content {
                for (id, name) in files {
                    if id == &file_id {
                        *name = filename.clone();
                    }
                }
            }
        }
    }
}

pub fn ui(f: &mut Frame, app: &mut App) {
    // Calculate input height based on content (min 3, max 10 lines)
    let input_lines = app.input.split('\n').count().max(1);
    let input_height = (input_lines + 2).clamp(3, 10) as u16; // +2 for borders

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(input_height)].as_ref())
        .split(f.area());

    render_messages(f, app, chunks[0]);
    render_input(f, app, chunks[1]);
}

fn render_messages(f: &mut Frame, app: &mut App, area: Rect) {
    // Build lines and calculate total wrapped lines
    let (lines, total_wrapped_lines) =
        build_message_lines(app, area.width.saturating_sub(4) as usize);

    let visible_lines = area.height.saturating_sub(2) as usize;

    // Update scroll bounds with actual wrapped line count
    app.update_scroll_bounds(total_wrapped_lines, visible_lines);

    // Create title
    let title = if app.selection_mode {
        "agnt (SELECTION MODE - Press Ctrl+S to exit)".to_string()
    } else if app.code_execution_enabled {
        "agnt (CODE EXECUTION ENABLED - Press Ctrl+X to disable)".to_string()
    } else {
        let mut title_parts = vec!["agnt".to_string()];

        // Add container info if present
        if let Some((id, _)) = &app.container_info {
            title_parts.push(format!("[Container: {}]", &id[..8]));
        }

        // Add scroll position if not auto-scrolling
        if !app.auto_scroll {
            title_parts.push(format!(
                "(Line {}/{})",
                app.scroll_position + 1,
                total_wrapped_lines
            ));
        }

        title_parts.join(" ")
    };

    // Create the messages paragraph with scrolling
    let messages = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: true })
        .scroll((app.scroll_position as u16, 0));

    f.render_widget(messages, area);
}

fn build_message_lines(app: &App, available_width: usize) -> (Vec<Line<'static>>, usize) {
    let mut lines: Vec<Line> = Vec::new();

    for (role, contents) in &app.messages {
        match role.as_str() {
            "user" => {
                // User message header
                lines.push(Line::from(vec![Span::styled(
                    "▶ You".to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]));

                // User message content
                for content in contents {
                    render_content(&mut lines, content, "  ");
                }
            }
            "assistant" => {
                // Claude message header
                lines.push(Line::from(vec![Span::styled(
                    "◆ Claude".to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]));

                // Claude message content
                for content in contents {
                    render_content(&mut lines, content, "  ");
                }
            }
            _ => {}
        }

        // Add spacing between messages
        lines.push(Line::from(""));
    }

    // Add streaming content if present
    if !app.streaming_content.is_empty() {
        // Streaming header
        lines.push(Line::from(vec![Span::styled(
            "◆ Claude".to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));

        if app.streaming_content.is_empty()
            || (app.streaming_content.len() == 1
                && matches!(&app.streaming_content[0], MessageContent::Text(t) if t.is_empty()))
        {
            lines.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(
                    "▸".to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::RAPID_BLINK),
                ),
            ]));
        } else {
            for content in &app.streaming_content {
                render_content(&mut lines, content, "  ");
            }
        }
        lines.push(Line::from(""));
    }

    // Remove trailing empty lines
    while lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }

    // Calculate actual wrapped lines
    let mut total_wrapped_lines = 0;
    for line in &lines {
        let line_text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        if line_text.is_empty() {
            total_wrapped_lines += 1;
        } else {
            // Calculate how many visual lines this logical line will occupy
            let line_width = line_text.chars().count();
            let wrapped_count = line_width.div_ceil(available_width);
            total_wrapped_lines += wrapped_count.max(1);
        }
    }

    (lines, total_wrapped_lines)
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let (input_title, border_color) = if app.selection_mode {
        (
            "Input (SELECTION MODE - text can be selected)",
            Color::Yellow,
        )
    } else if app.is_waiting {
        let waiting_text = if app.code_execution_enabled {
            "Input (waiting for response with code execution... Esc: cancel)"
        } else {
            "Input (waiting for response... Esc: cancel)"
        };
        (waiting_text, Color::DarkGray)
    } else {
        let border_color = if app.code_execution_enabled {
            Color::Magenta // Pink/red color for code execution
        } else {
            Color::Cyan
        };
        (
            "Input (Enter: send, Alt+Enter: newline, Ctrl+C: quit, Ctrl+S: selection, Ctrl+X: toggle code)",
            border_color,
        )
    };

    let input = Paragraph::new(app.input.as_str())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(Style::default().fg(border_color)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(input, area);

    // Calculate cursor position for multi-line input
    // Split by \n to handle trailing newlines properly
    let lines: Vec<&str> = app.input.split('\n').collect();
    let current_line = lines.len().saturating_sub(1);
    let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0);

    // Account for wrapped lines
    let available_width = area.width.saturating_sub(2) as usize; // -2 for borders
    let mut cursor_y = area.y + 1;

    for (i, line) in lines.iter().enumerate() {
        if i == current_line {
            break;
        }
        // Calculate wrapped lines for this line
        let wrapped_count = line.len().div_ceil(available_width).max(1);
        cursor_y += wrapped_count as u16;
    }

    // Calculate x position on the last line
    let cursor_x = area.x + 1 + (last_line_len % available_width) as u16;

    f.set_cursor_position((cursor_x, cursor_y));
}

fn render_content(lines: &mut Vec<Line<'static>>, content: &MessageContent, prefix: &str) {
    match content {
        MessageContent::Text(text) => {
            for line in text.lines() {
                lines.push(Line::from(vec![
                    Span::raw(prefix.to_string()),
                    Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
                ]));
            }
        }
        MessageContent::Code { input } => {
            // Code header
            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled("┌─ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "Python Code".to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Code content with line numbers
            for (idx, line) in input.lines().enumerate() {
                lines.push(Line::from(vec![
                    Span::raw(prefix.to_string()),
                    Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:3} ", idx + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(line.to_string(), Style::default().fg(Color::Blue)),
                ]));
            }

            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled("└─".to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
        MessageContent::CodeOutput {
            stdout,
            stderr,
            return_code,
            files,
        } => {
            // Output header
            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled("┌─ ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if *return_code == 0 {
                        "Output".to_string()
                    } else {
                        "Output (Error)".to_string()
                    },
                    Style::default()
                        .fg(if *return_code == 0 {
                            Color::Green
                        } else {
                            Color::Red
                        })
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Stdout
            if !stdout.is_empty() {
                for line in stdout.lines() {
                    lines.push(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(line.to_string(), Style::default().fg(Color::White)),
                    ]));
                }
            }

            // Stderr
            if !stderr.is_empty() {
                for line in stderr.lines() {
                    lines.push(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(line.to_string(), Style::default().fg(Color::Red)),
                    ]));
                }
            }

            // Files
            if !files.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(prefix.to_string()),
                    Span::styled("│ ".to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "Created files:".to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                for (file_id, filename) in files {
                    // If filename is the same as file_id, we're still waiting for metadata
                    let display_name = if filename == file_id {
                        format!("Loading... ({})", &file_id[..12.min(file_id.len())])
                    } else {
                        filename.clone()
                    };

                    lines.push(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::styled("│   • ".to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(display_name, Style::default().fg(Color::Blue)),
                        Span::styled(" (ID: ".to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            file_id[..8.min(file_id.len())].to_string(),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled("...)".to_string(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }

            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled("└─".to_string(), Style::default().fg(Color::DarkGray)),
            ]));
        }
        MessageContent::CodeError(error) => {
            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled(
                    "⚠ Code Execution Error: ".to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(error.to_string(), Style::default().fg(Color::Red)),
            ]));
        }
    }
}
