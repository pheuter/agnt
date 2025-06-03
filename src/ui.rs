use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolMode {
    None,
    CodeExecution,
    WebSearch,
    Both,
}

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
    ApiError(String),
}

#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub action: SlashCommandAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommandAction {
    Clear,
}

#[derive(Debug, Clone)]
pub struct SlashCommandState {
    pub input_buffer: String,
    pub suggestions: Vec<SlashCommand>,
    pub selected_index: usize,
}

impl SlashCommandState {
    pub fn new() -> Self {
        Self {
            input_buffer: String::new(),
            suggestions: Vec::new(),
            selected_index: 0,
        }
    }

    pub fn update_suggestions(&mut self, commands: &[SlashCommand]) {
        self.suggestions = commands
            .iter()
            .filter(|cmd| cmd.name.starts_with(&self.input_buffer))
            .cloned()
            .collect();
        self.selected_index = 0;
    }

    pub fn next_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.suggestions.len();
        }
    }

    pub fn prev_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.suggestions.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    pub fn get_selected(&self) -> Option<&SlashCommand> {
        self.suggestions.get(self.selected_index)
    }
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
    pub tool_mode: ToolMode,                    // Currently active tools
    pub loading_animation_frame: usize,         // Current frame of loading animation
    pub last_animation_update: std::time::Instant, // Time of last animation update
    pub connection_status: Option<String>,      // Current connection status
    pub show_help: bool,                        // Whether to show help modal
    pub slash_command_state: Option<SlashCommandState>, // Slash command autocomplete state
    pub available_commands: Vec<SlashCommand>,  // Available slash commands
    pub system_prompt: String,                  // System prompt for the AI
}

impl Default for App {
    fn default() -> Self {
        let available_commands = vec![SlashCommand {
            name: "clear".to_string(),
            description: "Clear the conversation history".to_string(),
            action: SlashCommandAction::Clear,
        }];

        let default_system_prompt = "You are a helpful assistant. Your knowledge cut-off is March 2025. The current date and time is [DATE_TIME_WITH_WEEKDAY_AND_TIMEZONE]".to_string();

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
            tool_mode: ToolMode::None,
            loading_animation_frame: 0,
            last_animation_update: std::time::Instant::now(),
            connection_status: None,
            show_help: false,
            slash_command_state: None,
            available_commands,
            system_prompt: default_system_prompt,
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
        self.loading_animation_frame = 0;
        self.last_animation_update = std::time::Instant::now();
        // Auto-scroll will be handled during rendering
    }

    pub fn update_loading_animation(&mut self) {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_animation_update).as_millis() >= 300 {
            self.loading_animation_frame = (self.loading_animation_frame + 1) % 3;
            self.last_animation_update = now;
        }
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

    pub fn add_api_error(&mut self, error: String) {
        self.messages
            .push(("system".to_string(), vec![MessageContent::ApiError(error)]));
    }

    pub fn set_container_info(&mut self, id: String, expires_at: String) {
        self.container_info = Some((id, expires_at));
    }

    pub fn set_connection_status(&mut self, status: Option<String>) {
        self.connection_status = status;
    }

    pub fn finish_streaming(&mut self) {
        if !self.streaming_content.is_empty() {
            let content = std::mem::take(&mut self.streaming_content);
            self.messages.push(("assistant".to_string(), content));
        }
        self.connection_status = None;
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
        self.tool_mode = match self.tool_mode {
            ToolMode::None => ToolMode::CodeExecution,
            ToolMode::CodeExecution => ToolMode::None,
            ToolMode::WebSearch => ToolMode::Both,
            ToolMode::Both => ToolMode::WebSearch,
        };
    }

    pub fn toggle_web_search(&mut self) {
        self.tool_mode = match self.tool_mode {
            ToolMode::None => ToolMode::WebSearch,
            ToolMode::WebSearch => ToolMode::None,
            ToolMode::CodeExecution => ToolMode::Both,
            ToolMode::Both => ToolMode::CodeExecution,
        };
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
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

    pub fn start_slash_command(&mut self) {
        let mut state = SlashCommandState::new();
        state.update_suggestions(&self.available_commands);
        self.slash_command_state = Some(state);
    }

    pub fn update_slash_command(&mut self, input: &str) {
        if let Some(state) = &mut self.slash_command_state {
            state.input_buffer = input.to_string();
            state.update_suggestions(&self.available_commands);
        }
    }

    pub fn cancel_slash_command(&mut self) {
        self.slash_command_state = None;
    }

    pub fn execute_slash_command(&mut self, action: SlashCommandAction) {
        match action {
            SlashCommandAction::Clear => {
                self.messages.clear();
                self.streaming_content.clear();
                self.scroll_position = 0;
                self.auto_scroll = true;
                self.total_lines = 0;
                self.container_info = None;
            }
        }
        self.slash_command_state = None;
        self.clear_input();
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

    // Render slash command autocomplete menu if active
    if let Some(state) = &app.slash_command_state {
        render_slash_command_menu(f, state, chunks[1]);
    }

    // Render help modal if active
    if app.show_help {
        render_help_modal(f);
    }
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
    } else {
        let mut title_parts = vec!["agnt".to_string()];

        // Add tool mode info
        let tool_info = match app.tool_mode {
            ToolMode::CodeExecution => "(CODE EXECUTION - Ctrl+X to toggle)",
            ToolMode::WebSearch => "(WEB SEARCH - Ctrl+W to toggle)",
            ToolMode::Both => "(CODE EXECUTION + WEB SEARCH - Ctrl+X/W to toggle)",
            ToolMode::None => "",
        };
        if !tool_info.is_empty() {
            title_parts.push(tool_info.to_string());
        }

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
            "system" => {
                // System messages (API errors, etc.) - render without header
                for content in contents {
                    render_content(&mut lines, content, "");
                }
            }
            _ => {}
        }

        // Add spacing between messages
        lines.push(Line::from(""));
    }

    // Add streaming content if present OR if waiting for response
    if !app.streaming_content.is_empty() || app.is_waiting {
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
            // Render loading animation
            let dots = match app.loading_animation_frame % 3 {
                0 => "●○○",
                1 => "○●○",
                2 => "○○●",
                _ => "●○○",
            };

            // Show connection status if available, otherwise show "Thinking..."
            let status_text = if let Some(ref status) = app.connection_status {
                format!(" {}", status)
            } else {
                " Thinking...".to_string()
            };

            lines.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(
                    dots.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    status_text,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
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
        let waiting_text = match app.tool_mode {
            ToolMode::CodeExecution => {
                "Input (waiting for response with code execution... Esc: cancel)"
            }
            ToolMode::WebSearch => "Input (waiting for response with web search... Esc: cancel)",
            ToolMode::Both => "Input (waiting for response with code + web search... Esc: cancel)",
            ToolMode::None => "Input (waiting for response... Esc: cancel)",
        };
        (waiting_text, Color::DarkGray)
    } else {
        let border_color = match app.tool_mode {
            ToolMode::CodeExecution | ToolMode::Both => Color::Magenta, // Pink/red color for code execution
            ToolMode::WebSearch => Color::Blue,                         // Blue for web search
            ToolMode::None => Color::Cyan,
        };
        ("Input (Ctrl+H: help, Ctrl+C: exit)", border_color)
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
        MessageContent::ApiError(error) => {
            lines.push(Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled(
                    "❌ API Error: ".to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(error.to_string(), Style::default().fg(Color::Red)),
            ]));
        }
    }
}

fn render_help_modal(f: &mut Frame) {
    let area = centered_rect(60, 80, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, area);

    // Create help content
    let help_text = vec![
        Line::from(vec![Span::styled(
            "agnt Help",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Message Input",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  Enter         ", Style::default().fg(Color::Magenta)),
            Span::styled("Send message", Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled("  Alt+Enter     ", Style::default().fg(Color::Magenta)),
            Span::styled("Insert newline", Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled("  Esc           ", Style::default().fg(Color::Magenta)),
            Span::styled(
                "Cancel streaming response",
                Style::default().fg(Color::Black),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  Page Up       ", Style::default().fg(Color::Magenta)),
            Span::styled("Scroll up 10 lines", Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled("  Page Down     ", Style::default().fg(Color::Magenta)),
            Span::styled("Scroll down 10 lines", Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled("  Mouse Wheel   ", Style::default().fg(Color::Magenta)),
            Span::styled("Scroll up/down 3 lines", Style::default().fg(Color::Black)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Modes",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  Ctrl+S        ", Style::default().fg(Color::Magenta)),
            Span::styled(
                "Toggle selection mode (for copying text)",
                Style::default().fg(Color::Black),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+X        ", Style::default().fg(Color::Magenta)),
            Span::styled(
                "Toggle code execution mode",
                Style::default().fg(Color::Black),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+W        ", Style::default().fg(Color::Magenta)),
            Span::styled("Toggle web search mode", Style::default().fg(Color::Black)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "General",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("  Ctrl+H        ", Style::default().fg(Color::Magenta)),
            Span::styled("Show/hide this help", Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C        ", Style::default().fg(Color::Magenta)),
            Span::styled("Quit agnt", Style::default().fg(Color::Black)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close this help",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Indexed(252))),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    f.render_widget(help, area);
}

fn render_slash_command_menu(f: &mut Frame, state: &SlashCommandState, input_area: Rect) {
    if state.suggestions.is_empty() {
        return;
    }

    // Calculate maximum width needed for the menu
    let max_cmd_width = state
        .suggestions
        .iter()
        .map(|cmd| cmd.name.len() + cmd.description.len() + 7) // +7 for "/ - " and some padding
        .max()
        .unwrap_or(20);

    // Calculate menu dimensions
    let menu_height = (state.suggestions.len() as u16 + 2).min(8); // +2 for borders, max 8 lines
    let menu_y = input_area.y.saturating_sub(menu_height);
    let menu_width = (max_cmd_width as u16 + 4).min(input_area.width.saturating_sub(2)); // +4 for padding and borders

    let menu_area = Rect {
        x: input_area.x,
        y: menu_y,
        width: menu_width,
        height: menu_height,
    };

    // Clear the area behind the menu
    f.render_widget(Clear, menu_area);

    // Add a subtle shadow effect
    let shadow_area = Rect {
        x: menu_area.x.saturating_add(1),
        y: menu_area.y.saturating_add(1),
        width: menu_area.width.saturating_sub(1),
        height: menu_area.height.saturating_sub(1),
    };

    if shadow_area.width > 0 && shadow_area.height > 0 {
        let shadow = Block::default().style(Style::default().bg(Color::Indexed(233))); // Very dark shadow
        f.render_widget(shadow, shadow_area);
    }

    // Create list items
    let items: Vec<ListItem> = state
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let is_selected = i == state.selected_index;

            let content = if is_selected {
                Line::from(vec![
                    Span::styled(
                        format!(" /{}", cmd.name),
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" - {} ", cmd.description),
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        format!("/{}", cmd.name),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" - ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&cmd.description, Style::default().fg(Color::Gray)),
                    Span::raw(" "),
                ])
            };

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title("┤ Commands ├")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Indexed(235))), // Very dark gray background
    );

    f.render_widget(list, menu_area);
}

// Helper function to center a rect
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
