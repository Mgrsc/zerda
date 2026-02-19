pub trait TruncateForUi {
    fn truncate_for_ui(&self, max_chars: usize) -> String;
}

impl TruncateForUi for str {
    fn truncate_for_ui(&self, max_chars: usize) -> String {
        if self.len() <= max_chars {
            return self.to_string();
        }
        let half = max_chars / 2;
        let head_end = self.floor_char_boundary(half);
        let tail_start = self.len() - half;
        let tail_start = {
            let mut i = tail_start.min(self.len());
            while i < self.len() && !self.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        let head = &self[..head_end];
        let tail = &self[tail_start..];
        format!(
            "{head}\n\n[... truncated {} chars ...]\n\n{tail}",
            self.len() - max_chars
        )
    }
}

impl TruncateForUi for String {
    fn truncate_for_ui(&self, max_chars: usize) -> String {
        self.as_str().truncate_for_ui(max_chars)
    }
}
