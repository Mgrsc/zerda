use std::fmt::Write;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: String,
}

#[derive(Default, serde::Deserialize)]
struct SkillMeta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

fn parse_skill_md(content: &str) -> SkillMeta {
    let trimmed = content.trim_start();
    let Some(after) = trimmed.strip_prefix("---") else {
        return SkillMeta::default();
    };
    let Some(end) = after.find("\n---") else {
        return SkillMeta::default();
    };
    serde_yaml::from_str(&after[..end]).unwrap_or_default()
}

pub fn load_skills(skills_dir: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();

    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let entry_path = entry.path();
        let dir_name = entry.file_name().to_string_lossy().to_string();

        let path = entry_path.join("SKILL.md");
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let meta = parse_skill_md(&content);

        let name = meta.name.unwrap_or_else(|| dir_name.clone());
        let description = meta.description.unwrap_or_else(|| {
            content
                .lines()
                .find(|line| {
                    !line.starts_with('#') && !line.starts_with("---") && !line.trim().is_empty()
                })
                .unwrap_or("")
                .to_string()
        });

        skills.push(Skill {
            name,
            description,
            path: entry_path.to_string_lossy().to_string(),
        });
    }

    skills
}

pub fn skills_index(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut index = String::from("<skills>\n");
    for skill in skills {
        let _ = writeln!(index, "- {}: {}", skill.name, skill.description);
    }
    index.push_str("</skills>");
    index
}
