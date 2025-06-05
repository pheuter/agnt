#[macro_use]
mod logger;
mod anthropic;
mod ui;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ui::{App, ToolMode};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run in pipe mode (read from stdin, write to stdout)
    #[arg(short, long)]
    pipe: bool,

    /// Optional prompt to prepend to piped input
    #[arg(short = 'm', long, value_name = "MESSAGE")]
    message: Option<String>,

    /// Enable code execution (requires Claude model that supports it)
    #[arg(short = 'x', long)]
    code_execution: bool,

    /// Enable web search (requires Claude model that supports it)
    #[arg(short = 'w', long)]
    web_search: bool,

    /// Directory to save files created by code execution (default: ./output when code execution is enabled)
    #[arg(short = 'o', long, value_name = "DIR")]
    output_dir: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logger and keep guard alive for the duration of the program
    let _logger_guard = match logger::init_logger() {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!("Warning: Could not create log file: {}", e);
            None
        }
    };

    // Set up panic hook to log termination on panic
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Try to log, but don't panic if logging fails
        log_debug!("=== AGNT Terminated (panic) ===");
        log_debug!("Panic info: {}", panic_info);

        // Call the default panic handler to get standard panic output
        default_panic(panic_info);
    }));

    log_debug!("=== AGNT Started ===");
    log_debug!("Args: {:?}", args);

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Error: ANTHROPIC_API_KEY environment variable is not set");
            eprintln!("Please set your Anthropic API key:");
            eprintln!("  export ANTHROPIC_API_KEY=your_api_key_here");
            return Ok(());
        }
    };

    let model =
        std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
    log_debug!("Initialized with ANTHROPIC_MODEL: {}", model);

    // Determine initial tool mode based on CLI flags
    let initial_tool_mode = match (args.code_execution, args.web_search) {
        (true, true) => ToolMode::Both,
        (true, false) => ToolMode::CodeExecution,
        (false, true) => ToolMode::WebSearch,
        (false, false) => ToolMode::None,
    };

    let client = anthropic::AnthropicClient::new(api_key).with_tool_mode(initial_tool_mode);

    // Default output directory to "output" if code execution is enabled and no dir specified
    let output_dir = if matches!(initial_tool_mode, ToolMode::CodeExecution | ToolMode::Both) {
        Some(args.output_dir.unwrap_or_else(|| "output".to_string()))
    } else {
        args.output_dir
    };

    let result = if args.pipe {
        // Pipe mode: read from stdin, send to API, write to stdout
        run_pipe_mode(client, args.message, output_dir).await
    } else {
        // Interactive TUI mode
        run_tui_mode(client, output_dir).await
    };

    log_debug!("=== AGNT Terminated ===");
    result
}

async fn run_pipe_mode(
    client: anthropic::AnthropicClient,
    prepend_message: Option<String>,
    output_dir: Option<String>,
) -> Result<()> {
    // Read input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Combine optional message with stdin input
    let full_message = match prepend_message {
        Some(msg) => format!("{} {}", msg, input),
        None => input,
    };

    // Create message and send to API
    let messages = vec![anthropic::Message {
        role: "user".to_string(),
        content: full_message,
    }];

    // Use default system prompt for pipe mode
    let default_prompt = "You are a helpful assistant. Your knowledge cut-off is March 2025. The current date and time is [DATE_TIME_WITH_WEEKDAY_AND_TIMEZONE]".to_string();
    let system_prompt = Some(substitute_datetime_placeholder(&default_prompt));
    let (mut receiver, _cancellation) = client.send_message_stream(messages, system_prompt).await?;

    // Stream response to stdout
    while let Some(event) = receiver.recv().await {
        match event {
            anthropic::StreamEvent::Text(text) => {
                print!("{}", text);
            }
            anthropic::StreamEvent::CodeInput(code) => {
                println!("\n```python\n{}\n```", code);
            }
            anthropic::StreamEvent::CodeOutput {
                stdout,
                stderr,
                return_code,
                files,
            } => {
                if !stdout.is_empty() {
                    println!("\nOutput:\n{}", stdout);
                }
                if !stderr.is_empty() {
                    eprintln!("\nError:\n{}", stderr);
                }
                if return_code != 0 {
                    eprintln!("(Exit code: {})", return_code);
                }
                if !files.is_empty() {
                    println!("\nCreated files:");
                    // If code execution is enabled, always save files (default to ./output)
                    let save_dir = output_dir.as_deref().unwrap_or("output");

                    for (file_id, filename) in &files {
                        println!("  - {} (ID: {})", filename, file_id);

                        // Save file locally if file ID is valid
                        if file_id.starts_with("file_") {
                            // Clone the client to use in the async block
                            let client_clone = client.clone();
                            let dir_clone = save_dir.to_string();
                            let file_id_clone = file_id.clone();

                            // Create a dummy channel for pipe mode (we don't update UI)
                            let (metadata_tx, _) = mpsc::channel::<(String, String)>(1);

                            // Spawn a task to download the file asynchronously
                            tokio::spawn(async move {
                                if let Err(e) = download_and_save_file(
                                    &client_clone,
                                    &dir_clone,
                                    &file_id_clone,
                                    metadata_tx,
                                )
                                .await
                                {
                                    log_debug!("Error saving file: {}", e);
                                }
                            });
                        } else {
                            eprintln!(
                                "Note: Cannot download file '{}' - file ID not available in streaming mode",
                                filename
                            );
                        }
                    }
                }
            }
            anthropic::StreamEvent::CodeError(error) => {
                eprintln!("\nCode execution error: {}", error);
            }
            anthropic::StreamEvent::ContainerInfo { .. } => {
                // Don't print container info in pipe mode
            }
            anthropic::StreamEvent::ConnectionStatus(_) => {
                // Don't print connection status in pipe mode
            }
        }
        use std::io::Write;
        io::stdout().flush()?;
    }
    println!(); // Add newline at end

    Ok(())
}

async fn run_tui_mode(
    client: anthropic::AnthropicClient,
    mut output_dir: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initially enable mouse capture
    execute!(terminal.backend_mut(), EnableMouseCapture)?;

    let mut app = App {
        tool_mode: client.tool_mode(),
        ..Default::default()
    };

    // If code execution is enabled but no output dir specified, default to "output"
    if matches!(app.tool_mode, ToolMode::CodeExecution | ToolMode::Both) && output_dir.is_none() {
        output_dir = Some("output".to_string());
    }

    let res = run_app(&mut terminal, &mut app, &client, output_dir).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    client: &anthropic::AnthropicClient,
    mut output_dir: Option<String>,
) -> Result<()> {
    // Remove the welcome message to keep the conversation clean

    let mut stream_receiver: Option<mpsc::Receiver<anthropic::StreamEvent>> = None;
    let mut stream_cancellation: Option<CancellationToken> = None;
    let (metadata_tx, mut metadata_rx) = mpsc::channel::<(String, String)>(100);

    loop {
        // Update loading animation if waiting
        if app.is_waiting {
            app.update_loading_animation();
        }

        terminal.draw(|f| ui::ui(f, app))?;

        // Handle file metadata updates
        if let Ok((file_id, filename)) = metadata_rx.try_recv() {
            app.update_file_metadata(file_id, filename);
        }

        // Handle streaming chunks
        if let Some(ref mut receiver) = stream_receiver {
            match receiver.try_recv() {
                Ok(event) => match event {
                    anthropic::StreamEvent::Text(text) => {
                        // Clear connection status once we start receiving content
                        app.set_connection_status(None);
                        app.append_streaming_text(&text);
                    }
                    anthropic::StreamEvent::CodeInput(code) => {
                        app.add_streaming_code(code);
                    }
                    anthropic::StreamEvent::CodeOutput {
                        stdout,
                        stderr,
                        return_code,
                        files,
                    } => {
                        // Save files locally whenever files are created
                        if !files.is_empty() {
                            // Always use default output directory if none specified
                            let dir = output_dir.as_deref().unwrap_or("output");

                            for (file_id, _filename) in &files {
                                // Only download files with valid file IDs
                                if file_id.starts_with("file_") {
                                    // Clone values for the async task
                                    let client_clone = client.clone();
                                    let dir_clone = dir.to_string();
                                    let file_id_clone = file_id.clone();
                                    let metadata_tx_clone = metadata_tx.clone();

                                    // Spawn download task to avoid blocking the UI
                                    tokio::spawn(async move {
                                        match download_and_save_file(
                                            &client_clone,
                                            &dir_clone,
                                            &file_id_clone,
                                            metadata_tx_clone,
                                        )
                                        .await
                                        {
                                            Err(e) => {
                                                log_debug!(
                                                    "Error saving file {}: {}",
                                                    file_id_clone,
                                                    e
                                                );
                                            }
                                            Ok(()) => {
                                                // Success is already logged in download_and_save_file
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        app.add_streaming_output(stdout, stderr, return_code, files);
                    }
                    anthropic::StreamEvent::CodeError(error) => {
                        app.add_streaming_error(error);
                    }
                    anthropic::StreamEvent::ContainerInfo { id, expires_at } => {
                        app.set_container_info(id, expires_at);
                    }
                    anthropic::StreamEvent::ConnectionStatus(status) => {
                        app.set_connection_status(Some(status.clone()));
                    }
                },
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Stream finished
                    app.finish_streaming();
                    app.is_waiting = false;
                    stream_receiver = None;
                    stream_cancellation = None;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No new chunks yet
                }
            }
        }

        // Use shorter poll timeout when animating
        let poll_timeout = if app.is_waiting {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // If help modal is shown, any key press closes it
                    if app.show_help {
                        app.toggle_help();
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('c')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            log_debug!("User requested termination with Ctrl+C");
                            return Ok(());
                        }
                        KeyCode::Char('h')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            app.toggle_help();
                        }
                        KeyCode::Esc => {
                            // Cancel slash command if active
                            if app.slash_command_state.is_some() {
                                app.cancel_slash_command();
                                app.clear_input();
                            } else if let Some(token) = stream_cancellation.take() {
                                // Cancel streaming if it's in progress
                                token.cancel();
                                // The stream will clean up on the next iteration
                            }
                        }
                        KeyCode::Tab => {
                            // Navigate slash command suggestions
                            if let Some(state) = &mut app.slash_command_state {
                                state.next_suggestion();
                            }
                        }
                        KeyCode::Down => {
                            // Navigate slash command suggestions
                            if let Some(state) = &mut app.slash_command_state {
                                state.next_suggestion();
                            }
                        }
                        KeyCode::Up => {
                            // Navigate slash command suggestions
                            if let Some(state) = &mut app.slash_command_state {
                                state.prev_suggestion();
                            }
                        }
                        KeyCode::Char('s')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            app.toggle_selection_mode();
                            if app.selection_mode {
                                // Disable mouse capture to allow text selection
                                execute!(terminal.backend_mut(), DisableMouseCapture)?;
                            } else {
                                // Re-enable mouse capture for scrolling
                                execute!(terminal.backend_mut(), EnableMouseCapture)?;
                            }
                        }
                        KeyCode::Char('x')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            app.toggle_code_execution();
                            // If code execution is enabled and output_dir is None, set it to default
                            if matches!(app.tool_mode, ToolMode::CodeExecution | ToolMode::Both)
                                && output_dir.is_none()
                            {
                                output_dir = Some("output".to_string());
                            }
                        }
                        KeyCode::Char('w')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            app.toggle_web_search();
                            // If code execution is enabled and output_dir is None, set it to default
                            if matches!(app.tool_mode, ToolMode::CodeExecution | ToolMode::Both)
                                && output_dir.is_none()
                            {
                                output_dir = Some("output".to_string());
                            }
                        }
                        KeyCode::Enter if key.modifiers.intersects(event::KeyModifiers::ALT) => {
                            app.input.push('\n');
                        }
                        KeyCode::Enter => {
                            // Handle slash command execution
                            if let Some(state) = &app.slash_command_state {
                                if let Some(cmd) = state.get_selected() {
                                    app.execute_slash_command(cmd.action.clone());
                                }
                            } else if !app.input.is_empty() && !app.is_waiting {
                                let user_message = app.input.clone();
                                app.clear_input();
                                app.add_message("user".to_string(), user_message.clone());
                                app.is_waiting = true;
                                app.auto_scroll = true; // Enable auto-scroll when sending a message
                                app.start_streaming();

                                // Force immediate redraw to show user message and streaming state
                                terminal.draw(|f| ui::ui(f, app))?;

                                let mut messages = vec![];
                                for (role, contents) in &app.messages {
                                    if role != "system" {
                                        // Convert MessageContent back to text for API
                                        let mut text_content = String::new();
                                        for content in contents {
                                            match content {
                                                ui::MessageContent::Text(text) => {
                                                    text_content.push_str(text);
                                                }
                                                _ => {
                                                    // Skip non-text content when building messages
                                                }
                                            }
                                        }
                                        if !text_content.is_empty() {
                                            messages.push(anthropic::Message {
                                                role: role.clone(),
                                                content: text_content,
                                            });
                                        }
                                    }
                                }

                                // Create a new client with the current tool settings
                                let client_with_tools =
                                    client.clone().with_tool_mode(app.tool_mode);

                                // send_message_stream now returns immediately with channel and cancellation token
                                let system_prompt =
                                    Some(substitute_datetime_placeholder(&app.system_prompt));
                                match client_with_tools
                                    .send_message_stream(messages, system_prompt)
                                    .await
                                {
                                    Ok((receiver, cancellation)) => {
                                        stream_receiver = Some(receiver);
                                        stream_cancellation = Some(cancellation);
                                    }
                                    Err(e) => {
                                        // This should rarely happen now as most errors are sent through the channel
                                        app.finish_streaming();
                                        app.add_api_error(format!(
                                            "Failed to start request: {}",
                                            e
                                        ));
                                        app.is_waiting = false;
                                    }
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            // Check if starting a slash command
                            if c == '/' && app.input.is_empty() && !app.is_waiting {
                                app.input.push(c);
                                app.start_slash_command();
                            } else if app.slash_command_state.is_some() {
                                app.input.push(c);
                                let input_copy = app.input.clone();
                                app.update_slash_command(&input_copy[1..]); // Skip the '/'
                            } else {
                                app.input.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            app.input.pop();

                            // Update or cancel slash command state
                            if app.slash_command_state.is_some() {
                                if app.input.is_empty() {
                                    app.cancel_slash_command();
                                } else {
                                    let input_copy = app.input.clone();
                                    app.update_slash_command(&input_copy[1..]); // Skip the '/'
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            app.scroll_up(10);
                        }
                        KeyCode::PageDown => {
                            app.scroll_down(10);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    // Only handle mouse events when not in selection mode
                    if !app.selection_mode {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                app.scroll_up(3);
                            }
                            MouseEventKind::ScrollDown => {
                                app.scroll_down(3);
                            }
                            _ => {}
                        }
                    }
                }
                Event::Resize(_, _) => {
                    terminal.clear()?;
                }
                _ => {}
            }
        }
    }
}

async fn download_and_save_file(
    client: &anthropic::AnthropicClient,
    output_dir: &str,
    file_id: &str,
    metadata_tx: mpsc::Sender<(String, String)>,
) -> Result<()> {
    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir)?;

    // First, try to get the actual filename from the metadata API
    let actual_filename = match client.get_file_metadata(file_id).await {
        Ok(metadata) => {
            let filename = metadata.filename;
            // Send metadata update to UI
            let _ = metadata_tx
                .send((file_id.to_string(), filename.clone()))
                .await;
            filename
        }
        Err(e) => {
            log_debug!(
                "Warning: Could not fetch file metadata for {}: {}",
                file_id,
                e
            );
            // Add a small delay and retry once in case the file isn't ready yet
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            match client.get_file_metadata(file_id).await {
                Ok(metadata) => {
                    let filename = metadata.filename;
                    // Send metadata update to UI
                    let _ = metadata_tx
                        .send((file_id.to_string(), filename.clone()))
                        .await;
                    filename
                }
                Err(_) => format!("{}.bin", file_id),
            }
        }
    };

    // Sanitize filename to prevent path traversal and clean special characters
    let safe_filename = Path::new(&actual_filename)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("unnamed_file"))
        .to_string_lossy();

    // Further clean the filename - replace problematic characters but keep the file extension
    let cleaned_filename = safe_filename
        .chars()
        .map(|c| {
            match c {
                // Keep alphanumeric, dots, hyphens, and underscores
                c if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' => c,
                // Replace spaces with underscores
                ' ' => '_',
                // Replace other characters with underscores
                _ => '_',
            }
        })
        .collect::<String>();

    let filepath = Path::new(output_dir).join(&cleaned_filename);

    // Try to download the actual file content
    match client.download_file(file_id).await {
        Ok(content) => {
            // Write the actual file content
            let mut file = fs::File::create(&filepath)?;
            file.write_all(&content)?;
            log_debug!(
                "Downloaded: {}",
                filepath
                    .canonicalize()
                    .unwrap_or(filepath.clone())
                    .display()
            );
        }
        Err(e) => {
            // If download fails, create a placeholder file with error information
            let mut file = fs::File::create(&filepath)?;
            writeln!(
                file,
                "Failed to download file from Claude's code execution.\n\
                \n\
                File ID: {}\n\
                Error: {}\n\
                \n\
                This could be due to:\n\
                - The file API not being available yet\n\
                - The file having expired\n\
                - Authentication or permission issues\n\
                \n\
                You can try using the Anthropic Files API directly with the file ID above.",
                file_id, e
            )?;
            log_debug!(
                "Warning: Could not download file content, created placeholder instead: {}",
                e
            );
        }
    }

    Ok(())
}

fn substitute_datetime_placeholder(prompt: &str) -> String {
    use chrono::{Datelike, Local, Timelike};

    let now = Local::now();
    let weekday = match now.weekday() {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    };

    let datetime_str = format!(
        "{}, {} {}, {} at {:02}:{:02}:{:02} {}",
        weekday,
        now.format("%B"),
        now.day(),
        now.year(),
        now.hour12().1,
        now.minute(),
        now.second(),
        if now.hour12().0 { "PM" } else { "AM" }
    );

    let timezone = now.format("%Z").to_string();
    let full_datetime = format!("{} {}", datetime_str, timezone);

    prompt.replace("[DATE_TIME_WITH_WEEKDAY_AND_TIMEZONE]", &full_datetime)
}
