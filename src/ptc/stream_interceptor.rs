pub struct PtcStreamInterceptor {
    hidden_mode: bool,
    pending: String,
    visible: String,
    hidden: String,
}

impl PtcStreamInterceptor {
    pub fn new() -> Self {
        Self {
            hidden_mode: false,
            pending: String::new(),
            visible: String::new(),
            hidden: String::new(),
        }
    }

    pub fn push(&mut self, chunk: &str) -> String {
        if chunk.is_empty() {
            return String::new();
        }

        if self.hidden_mode {
            self.hidden.push_str(chunk);
            return String::new();
        }

        self.pending.push_str(chunk);
        let tags = ["<PTC_TOOL_CALLING"];
        if let Some((position, _)) = find_first_tag(&self.pending, &tags) {
            let visible = self.pending[..position].to_string();
            if !visible.is_empty() {
                self.visible.push_str(&visible);
            }
            self.hidden.push_str(&self.pending[position..]);
            self.pending.clear();
            self.hidden_mode = true;
            return visible;
        }

        let retain = longest_prefix_suffix_any(&self.pending, &tags);
        let emit_len = self.pending.len().saturating_sub(retain);
        if emit_len == 0 {
            return String::new();
        }
        let emitted = self.pending[..emit_len].to_string();
        self.visible.push_str(&emitted);
        let suffix = self.pending[emit_len..].to_string();
        self.pending = suffix;
        emitted
    }

    pub fn finish(mut self) -> (String, String) {
        if !self.hidden_mode && !self.pending.is_empty() {
            self.visible.push_str(&self.pending);
            self.pending.clear();
        }
        (self.visible, self.hidden)
    }
}

fn find_first_tag<'a>(text: &'a str, tags: &[&'a str]) -> Option<(usize, &'a str)> {
    tags.iter()
        .filter_map(|tag| text.find(tag).map(|position| (position, *tag)))
        .min_by_key(|(position, _)| *position)
}

fn longest_prefix_suffix_any(text: &str, prefixes: &[&str]) -> usize {
    let mut best = 0;
    for prefix in prefixes {
        let max = text.len().min(prefix.len().saturating_sub(1));
        for len in (1..=max).rev() {
            if text.ends_with(&prefix[..len]) {
                best = best.max(len);
                break;
            }
        }
    }
    best
}
