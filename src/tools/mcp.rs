use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use rmcp::{
    model::{CallToolRequestParams, RawContent},
    service::{RoleClient, RunningService},
    transport::{
        child_process::{ConfigureCommandExt, TokioChildProcess},
        StreamableHttpClientTransport,
    },
    ServiceExt,
};
use serde_json::Value;
use tokio::sync::RwLock;

use super::{Tool, ToolResult};
use crate::config::McpServerConfig;

type McpClient = Arc<RunningService<RoleClient, ()>>;

pub struct McpToolAdapter {
    tool_name: String,
    description: String,
    schema: Value,
    client: Arc<RwLock<McpClient>>,
    config: Arc<McpServerConfig>,
}

impl McpToolAdapter {
    async fn reconnect(&self) -> Result<McpClient> {
        tracing::info!("Reconnecting MCP server: {}", self.config.name);
        let client = connect_client(&self.config).await?;
        let new_client = Arc::new(client);
        *self.client.write().await = Arc::clone(&new_client);
        Ok(new_client)
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        let params = CallToolRequestParams {
            name: self.tool_name.clone().into(),
            arguments: Some(args.as_object().cloned().unwrap_or_default()),
            meta: None,
            task: None,
        };

        let client = self.client.read().await.clone();
        let result = match client.call_tool(params.clone()).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "MCP tool {} failed, attempting reconnect: {e}",
                    self.tool_name
                );
                let new_client = self.reconnect().await.map_err(|re| {
                    anyhow::anyhow!("MCP call failed ({e}) and reconnect also failed ({re})")
                })?;
                new_client.call_tool(params).await?
            }
        };

        let text = result
            .content
            .iter()
            .filter_map(|c| match &c.raw {
                RawContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolResult {
            output: text,
            is_error: result.is_error.unwrap_or(false),
        })
    }
}

pub async fn connect_mcp_servers(configs: &[McpServerConfig]) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    for config in configs {
        match connect_single_server(config).await {
            Ok(server_tools) => tools.extend(server_tools),
            Err(e) => {
                tracing::warn!(server = %config.name, error = %e, "failed to connect MCP server")
            }
        }
    }

    tools
}

async fn connect_client(config: &McpServerConfig) -> Result<RunningService<RoleClient, ()>> {
    match config.transport.as_str() {
        "stdio" => {
            let cmd = config
                .command
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("stdio transport requires 'command'"))?;
            let args = config.args.clone();
            let env = config.env.clone();
            let transport =
                TokioChildProcess::new(tokio::process::Command::new(cmd).configure(|c| {
                    c.args(&args);
                    c.envs(&env);
                    c.kill_on_drop(true);
                }))?;
            Ok(().serve(transport).await?)
        }
        "streamable-http" => {
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("streamable-http transport requires 'url'"))?;
            let transport = StreamableHttpClientTransport::from_uri(url);
            Ok(().serve(transport).await?)
        }
        other => anyhow::bail!("unsupported MCP transport: {other}"),
    }
}

async fn connect_single_server(config: &McpServerConfig) -> Result<Vec<Box<dyn Tool>>> {
    let client = connect_client(config).await?;
    let client = Arc::new(client);
    let shared_client = Arc::new(RwLock::new(Arc::clone(&client)));
    let shared_config = Arc::new(config.clone());
    let listed = client.list_all_tools().await?;

    let tools: Vec<Box<dyn Tool>> = listed
        .into_iter()
        .map(|t| {
            let adapter = McpToolAdapter {
                tool_name: t.name.into_owned(),
                description: t
                    .description
                    .map(std::borrow::Cow::into_owned)
                    .unwrap_or_default(),
                schema: Value::Object(t.input_schema.as_ref().clone()),
                client: Arc::clone(&shared_client),
                config: Arc::clone(&shared_config),
            };
            Box::new(adapter) as Box<dyn Tool>
        })
        .collect();

    Ok(tools)
}
