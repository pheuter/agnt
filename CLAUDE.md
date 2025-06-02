# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`agnt` is a Rust-based terminal chat client for Anthropic's Claude API with advanced features including code execution support and a sophisticated TUI (Terminal User Interface).

## Essential Commands

```bash
# Build the project
cargo build
cargo build --release  # For optimized binary

# Run the application
cargo run              # Interactive TUI mode
cargo run -- --help    # Show command options
echo "prompt" | cargo run  # Pipe mode

# Development
cargo check            # Fast type checking
cargo clippy          # Linting
cargo fmt             # Format code
```

## Architecture & Key Components

### Core Modules

- **main.rs**: Entry point handling CLI arguments and mode selection (TUI vs pipe mode)
- **anthropic.rs**: Streaming API client for Claude with code execution tool support
- **ui.rs**: Terminal UI implementation using ratatui, handles real-time streaming, code highlighting, and user interactions
- **logger.rs**: Debug logging system writing to ~/.agnt/logs.txt

### Key Design Patterns

1. **Streaming Architecture**: Uses Server-Sent Events (SSE) for real-time Claude responses
2. **Event-Driven UI**: Terminal UI updates asynchronously as stream events arrive
3. **Tool Integration**: Supports code execution with automatic file saving and container management
4. **State Management**: Maintains conversation history, scroll position, and UI state in the App struct

### Environment Configuration

- **Required**: `ANTHROPIC_API_KEY` - Your Anthropic API key
- **Optional**: `ANTHROPIC_MODEL` - Model to use (defaults to claude-sonnet-4-20250514)

### Important Implementation Details

- Uses Rust edition 2024 features
- Async runtime via tokio with multi-threaded scheduler
- Error handling uses anyhow for ergonomic error propagation
- File operations use Anthropic's Files API for secure code execution
