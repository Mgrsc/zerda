use crate::providers::ConversationMessage;

pub struct Guideline {
    pub id: String,
    pub guideline_text: String,
    #[allow(dead_code)]
    pub score: f32,
}

pub struct IterationOutcome {
    pub had_tool_error: bool,
    pub had_traceback: bool,
}

pub struct ReflectionContext {
    pub instruction: String,
    pub history: Vec<ConversationMessage>,
    pub iteration_outcomes: Vec<IterationOutcome>,
    pub final_failed: bool,
    pub injected_guideline_ids: Vec<String>,
}
