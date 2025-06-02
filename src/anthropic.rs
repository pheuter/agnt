use anyhow::Result;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    api_key: String,
    client: Client,
    enable_code_execution: bool,
}

#[derive(Debug, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct Tool {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEventData {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStartData },
    #[serde(rename = "content_block_start")]
    ContentBlockStart { content_block: ContentBlock },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: Delta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop,
    #[serde(rename = "message_delta")]
    MessageDelta,
    #[serde(rename = "message_stop")]
    MessageStop,
}

#[derive(Debug, Deserialize)]
pub struct MessageStartData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<Container>,
}

#[derive(Debug, Deserialize)]
pub struct Container {
    pub id: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        #[allow(dead_code)]
        text: String,
    },
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        #[allow(dead_code)]
        id: String,
        name: String,
    },
    #[serde(rename = "code_execution_tool_result")]
    CodeExecutionToolResult {
        #[allow(dead_code)]
        tool_use_id: String,
        content: CodeExecutionResult,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum FileOutput {
    #[serde(rename = "code_execution_output")]
    CodeExecutionOutput { file_id: String },
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct FileMetadata {
    pub id: String,
    pub filename: String,
    #[serde(rename = "size_bytes")]
    pub size: u64,
    #[serde(rename = "mime_type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloadable: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ListFilesResponse {
    pub data: Vec<FileMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum CodeExecutionResult {
    #[serde(rename = "code_execution_result")]
    Success {
        stdout: String,
        stderr: String,
        return_code: i32,
        #[serde(default)]
        content: Vec<FileOutput>,
    },
    #[serde(rename = "code_execution_tool_result_error")]
    Error { error_code: String },
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Text(String),
    CodeInput(String),
    CodeOutput {
        stdout: String,
        stderr: String,
        return_code: i32,
        files: Vec<(String, String)>, // (file_id, filename)
    },
    CodeError(String),
    ContainerInfo {
        id: String,
        expires_at: String,
    },
    ConnectionStatus(String),
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
            enable_code_execution: false,
        }
    }

    pub fn with_code_execution(mut self, enable: bool) -> Self {
        self.enable_code_execution = enable;
        self
    }

    pub fn is_code_execution_enabled(&self) -> bool {
        self.enable_code_execution
    }

    pub async fn send_message_stream(
        &self,
        messages: Vec<Message>,
    ) -> Result<(mpsc::Receiver<StreamEvent>, CancellationToken)> {
        let (tx, rx) = mpsc::channel(100);
        let cancellation_token = CancellationToken::new();
        let token_clone = cancellation_token.clone();

        // Clone necessary data for the spawned task
        let api_key = self.api_key.clone();
        let client = self.client.clone();
        let enable_code_execution = self.enable_code_execution;

        // Spawn the entire request handling as a separate task
        tokio::spawn(async move {
            // Send initial connection status
            let _ = tx
                .send(StreamEvent::ConnectionStatus(
                    "Connecting to Claude API...".to_string(),
                ))
                .await;

            // Build the request
            let tools = if enable_code_execution {
                Some(vec![Tool {
                    tool_type: "code_execution_20250522".to_string(),
                    name: "code_execution".to_string(),
                }])
            } else {
                None
            };

            let model = std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

            let request = MessagesRequest {
                model,
                messages,
                max_tokens: 4096,
                stream: true,
                tools,
            };

            let mut request_builder = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json");

            if enable_code_execution {
                request_builder = request_builder.header(
                    "anthropic-beta",
                    "code-execution-2025-05-22,files-api-2025-04-14",
                );
            }

            // Send the request (this is now in the spawned task)
            let _ = tx
                .send(StreamEvent::ConnectionStatus(
                    "Sending request...".to_string(),
                ))
                .await;
            let response = match request_builder.json(&request).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    log_debug!("Failed to send request to Messages API: {}", e);
                    if e.to_string().contains("dns") || e.to_string().contains("connect") {
                        log_debug!("Network/connection error detected");
                    } else if e.to_string().contains("timed out") {
                        log_debug!("Request timeout error");
                    }
                    // Send error through the channel
                    let error_msg = format!("Failed to connect to Anthropic API: {}", e);
                    let _ = tx
                        .send(StreamEvent::Text(format!("\n\nError: {}\n", error_msg)))
                        .await;
                    return;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_else(|e| {
                    log_debug!("Failed to read error response body: {}", e);
                    "Failed to read error response".to_string()
                });

                log_debug!("API error response (status {}): {}", status, error_text);

                // Parse specific error types and send through channel
                let error_msg = if status == 401 {
                    log_debug!("Authentication error - invalid or missing API key");
                    format!("Invalid or missing API key: {}", error_text)
                } else if status == 400 {
                    if error_text.contains("model") {
                        log_debug!("Invalid model name error");
                        format!("Invalid model name: {}", error_text)
                    } else {
                        log_debug!("Bad request error");
                        format!("Bad request: {}", error_text)
                    }
                } else if status == 429 {
                    log_debug!("Rate limit error");
                    format!("Rate limit exceeded: {}", error_text)
                } else if status.is_server_error() {
                    log_debug!("Server error ({})", status);
                    format!("Anthropic server error: {}", error_text)
                } else {
                    format!("API error ({}): {}", status, error_text)
                };

                let _ = tx
                    .send(StreamEvent::Text(format!("\n\nError: {}\n", error_msg)))
                    .await;
                return;
            }

            // Process the streaming response
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut current_code_input = String::new();
            let mut collecting_code = false;

            loop {
                tokio::select! {
                    _ = token_clone.cancelled() => {
                        // Streaming was cancelled
                        break;
                    }
                    chunk = stream.next() => {
                        match chunk {
                            Some(Ok(bytes)) => {
                                if let Ok(text) = std::str::from_utf8(&bytes) {
                                    buffer.push_str(text);

                                    // Process complete SSE events
                                    while let Some(event_end) = buffer.find("\n\n") {
                                            let event_data = buffer[..event_end].to_string();
                                            buffer = buffer[event_end + 2..].to_string();

                                            // Parse SSE event
                                            if let Some(data_line) =
                                                event_data.lines().find(|line| line.starts_with("data: "))
                                            {
                                                let json_str = &data_line[6..];


                                                if let Ok(event) = serde_json::from_str::<StreamEventData>(json_str) {
                                                    match event {
                                                    StreamEventData::MessageStart { message } => {
                                                        if let Some(container) = message.container {
                                                            let _ = tx.send(StreamEvent::ContainerInfo {
                                                                id: container.id,
                                                                expires_at: container.expires_at,
                                                            }).await;
                                                        }
                                                    }
                                                    StreamEventData::ContentBlockStart { content_block } => {
                                                        match content_block {
                                                            ContentBlock::ServerToolUse { name, .. } => {
                                                                if name == "code_execution" {
                                                                    collecting_code = true;
                                                                    current_code_input.clear();
                                                                }
                                                            }
                                                            ContentBlock::CodeExecutionToolResult { content, .. } => {
                                                                match content {
                                                                    CodeExecutionResult::Success { stdout, stderr, return_code, content } => {
                                                                        // Extract files from the content array
                                                                        let files: Vec<(String, String)> = content.iter()
                                                                            .map(|f| match f {
                                                                                FileOutput::CodeExecutionOutput { file_id } => {
                                                                                    // Use file ID as both ID and temporary filename
                                                                                    // The UI will show just the file ID to avoid duplicate "file_file" prefix
                                                                                    (file_id.clone(), file_id.clone())
                                                                                }
                                                                            })
                                                                            .collect();


                                                                        let _ = tx.send(StreamEvent::CodeOutput {
                                                                            stdout,
                                                                            stderr,
                                                                            return_code,
                                                                            files,
                                                                        }).await;
                                                                    }
                                                                    CodeExecutionResult::Error { error_code } => {
                                                                        let _ = tx.send(StreamEvent::CodeError(error_code)).await;
                                                                    }
                                                                }
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    StreamEventData::ContentBlockDelta { delta } => {
                                                        match delta {
                                                            Delta::TextDelta { text } => {
                                                                if tx.send(StreamEvent::Text(text)).await.is_err() {
                                                                    break; // Exit if receiver dropped
                                                                }
                                                            }
                                                            Delta::InputJsonDelta { partial_json } => {
                                                                if collecting_code {
                                                                    current_code_input.push_str(&partial_json);
                                                                }
                                                            }
                                                        }
                                                    }
                                                    StreamEventData::ContentBlockStop => {
                                                        if collecting_code && !current_code_input.is_empty() {
                                                            // Extract code from JSON
                                                            if let Ok(json) = serde_json::from_str::<Value>(&current_code_input) {
                                                                if let Some(code) = json.get("code").and_then(|v| v.as_str()) {
                                                                    let _ = tx.send(StreamEvent::CodeInput(code.to_string())).await;
                                                                }
                                                            }
                                                            collecting_code = false;
                                                            current_code_input.clear();
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            }
                                        }
                                }
                            }
                            Some(Err(_)) | None => break,
                        }
                    }
                }
            }
        });

        Ok((rx, cancellation_token))
    }

    pub async fn get_file_metadata(&self, file_id: &str) -> Result<FileMetadata> {
        log_debug!("Fetching metadata for file: {}", file_id);

        let response = match self
            .client
            .get(format!("https://api.anthropic.com/v1/files/{}", file_id))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "files-api-2025-04-14")
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log_debug!("Failed to fetch file metadata: {}", e);
                return Err(anyhow::anyhow!("Failed to fetch file metadata: {}", e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|e| {
                log_debug!("Failed to read file metadata error response: {}", e);
                "Failed to read error response".to_string()
            });
            log_debug!(
                "File metadata API error (status {}): {}",
                status,
                error_text
            );
            return Err(anyhow::anyhow!(
                "Failed to get file metadata: {}",
                error_text
            ));
        }

        let response_text = response.text().await.map_err(|e| {
            log_debug!("Failed to read file metadata response body: {}", e);
            anyhow::anyhow!("Failed to read response: {}", e)
        })?;

        let metadata: FileMetadata = serde_json::from_str(&response_text).map_err(|e| {
            log_debug!("Failed to parse file metadata JSON: {}", e);
            log_debug!("Raw JSON: {}", response_text);
            anyhow::anyhow!("Failed to parse file metadata: {}", e)
        })?;

        log_debug!(
            "File metadata: {} ({}, {} bytes)",
            metadata.filename,
            metadata.content_type,
            metadata.size
        );

        Ok(metadata)
    }

    pub async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        log_debug!("Downloading file: {}", file_id);

        let response = match self
            .client
            .get(format!(
                "https://api.anthropic.com/v1/files/{}/content",
                file_id
            ))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "files-api-2025-04-14")
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log_debug!("Failed to download file: {}", e);
                return Err(anyhow::anyhow!("Failed to download file: {}", e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|e| {
                log_debug!("Failed to read file download error response: {}", e);
                "Failed to read error response".to_string()
            });
            log_debug!(
                "File download API error (status {}): {}",
                status,
                error_text
            );
            return Err(anyhow::anyhow!("Failed to download file: {}", error_text));
        }

        let content = response.bytes().await.map_err(|e| {
            log_debug!("Failed to read file content: {}", e);
            anyhow::anyhow!("Failed to read file content: {}", e)
        })?;

        log_debug!("Successfully downloaded {} bytes", content.len());
        Ok(content.to_vec())
    }

    #[allow(dead_code)]
    pub async fn list_files(&self) -> Result<ListFilesResponse> {
        let response = self
            .client
            .get("https://api.anthropic.com/v1/files")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "files-api-2025-04-14")
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Failed to list files: {}", error_text));
        }

        let files_response: ListFilesResponse = response.json().await?;
        Ok(files_response)
    }
}
