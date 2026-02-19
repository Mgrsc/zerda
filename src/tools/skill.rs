use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::RwLock;

use super::{Tool, ToolResult};
use crate::skills::Skill;

pub struct SkillTool {
    skills: Arc<RwLock<Vec<Skill>>>,
    content_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl SkillTool {
    pub fn new(
        skills: Arc<RwLock<Vec<Skill>>>,
        content_cache: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self {
            skills,
            content_cache,
        }
    }

    async fn get_skill_content(&self, skill: &Skill) -> Result<String, String> {
        {
            let cache = self.content_cache.read().await;
            if let Some(content) = cache.get(&skill.name) {
                return Ok(content.clone());
            }
        }

        let skill_md_path = std::path::Path::new(&skill.path).join("SKILL.md");
        match std::fs::read_to_string(&skill_md_path) {
            Ok(content) => {
                self.content_cache
                    .write()
                    .await
                    .insert(skill.name.clone(), content.clone());
                Ok(content)
            }
            Err(e) => Err(format!("Failed to read SKILL.md for '{}': {e}", skill.name)),
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Activate a skill by name to retrieve its full instructions"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name to activate"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments to pass to the skill"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let skill_name = args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let skill_args = args
            .get("args")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let skills = self.skills.read().await;
        let Some(skill) = skills.iter().find(|s| s.name == skill_name) else {
            return Ok(ToolResult {
                output: format!("Skill '{skill_name}' not found"),
                is_error: true,
            });
        };
        let skill = skill.clone();
        drop(skills);

        let content = match self.get_skill_content(&skill).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    output: e,
                    is_error: true,
                });
            }
        };

        let processed = if content.contains("$ARGUMENTS") {
            content.replace("$ARGUMENTS", skill_args)
        } else if !skill_args.is_empty() {
            format!("{content}\n\nARGUMENTS: {skill_args}")
        } else {
            content
        };

        Ok(ToolResult {
            output: format!("{processed}\n\n---\nSkill directory: {}", skill.path),
            is_error: false,
        })
    }
}
