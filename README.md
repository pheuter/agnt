# agnt

A simple terminal agent for Anthropic's Claude API with support for [code execution](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/code-execution-tool)

## Features

- **Interactive TUI Mode**: Minimal terminal interface with real-time streaming responses
- **Pipe Mode**: Simple command-line interface for scripting and automation
- **Code Execution**: Execute Python code in a secure, sandboxed environment managed by Anthropic
- **Conversation History**: Maintains full chat history with scrolling support
- **Selection Mode**: Copy text directly from the terminal interface

## Installation

### Homebrew

```bash
brew tap pheuter/tap
brew install agnt
```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/pheuter/agnt.git
cd agnt

# Build the project
cargo build --release

# The binary will be available at ./target/release/agnt
```

## Configuration

Set your Anthropic API key as an environment variable:

```bash
export ANTHROPIC_API_KEY="your-api-key-here"
```

Optionally, you can specify a different Claude model:

```bash
export ANTHROPIC_MODEL="claude-sonnet-4-20250514"  # Default
```

## Usage

### Interactive TUI Mode

Simply run the binary to start the interactive terminal interface:

```bash
agnt
```

**TUI Keybindings:**

- `Enter` - Send message
- `Alt+Enter` - Insert newline (multi-line input)
- `Ctrl+C` - Exit application
- `Ctrl+S` - Toggle selection mode (for copying text)
- `Ctrl+X` - Toggle code execution on/off
- `Esc` - Cancel streaming response
- `Mouse Scroll` - Scroll conversation (when not in selection mode)

### Pipe Mode

For scripting and automation, pipe input to agnt:

```bash
# Basic usage
echo "Explain quantum computing" | agnt --pipe

# With prepended message
cat file.txt | agnt --pipe --message "Analyze this file:"

# With code execution enabled
echo "Write a Python script to calculate fibonacci numbers" | agnt --pipe --code-execution
```

### Command-Line Options

```bash
agnt --help                              # Show help
agnt --pipe                              # Run in pipe mode
agnt --message "prompt"                  # Prepend message to piped input
agnt --code-execution                    # Enable code execution
agnt --output-dir ./my-output            # Set output directory for files (default: ./output)
```

**Available flags:**

- `-p, --pipe` - Run in pipe mode (read from stdin, write to stdout)
- `-m, --message <MESSAGE>` - Optional prompt to prepend to piped input
- `-x, --code-execution` - Enable code execution (requires compatible Claude model)
- `-o, --output-dir <DIR>` - Directory to save files created by code execution

## Architecture

The project is organized into four main modules:

- **main.rs**: CLI argument parsing and mode selection
- **anthropic.rs**: Streaming API client implementation
- **ui.rs**: Terminal UI with ratatui
- **logger.rs**: Debug logging system

## Development

```bash
# Run in development mode
cargo run

# Type checking
cargo check

# Linting
cargo clippy

# Format code
cargo fmt
```

## Logging

Debug logs are written to `~/.agnt/logs.txt` for troubleshooting. The log file is automatically recreated on each run.
