use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use super::{Channel, ChannelMessage, ChannelMessageOrigin};

pub struct CliChannel;

pub fn sanitize_terminal_text(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'[' => {
                    i += 1;
                    while i < bytes.len() {
                        let c = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&c) {
                            break;
                        }
                    }
                }
                b']' => {
                    i += 1;
                    while i < bytes.len() {
                        let c = bytes[i];
                        if c == 0x07 {
                            i += 1;
                            break;
                        }
                        if c == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                b'P' | b'X' | b'^' | b'_' => {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
            continue;
        }
        if (b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t') || b == 0x7f {
            i += 1;
            continue;
        }
        let ch = input[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    async fn send(&self, message: &str, _recipient: &str) -> Result<()> {
        println!("{}", sanitize_terminal_text(message));
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "/quit" || trimmed == "/exit" {
                break;
            }
            if let Err(e) = tx
                .send(ChannelMessage {
                    sender: "user".to_string(),
                    session_id: "cli-user".to_string(),
                    content: trimmed,
                    content_parts: None,
                    channel: "cli".to_string(),
                    origin: ChannelMessageOrigin::Human,
                    related_job_id: None,
                })
                .await
            {
                return Err(anyhow::anyhow!("CLI receiver channel closed: {e}"));
            }
        }
        Ok(())
    }
}
