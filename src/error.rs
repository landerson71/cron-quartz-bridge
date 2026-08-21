use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

/// A parse or conversion failure tied to an exact spot in the source line,
/// so it can be rendered with a caret the way a compiler would.
#[derive(Debug, Clone)]
pub struct CronError {
    pub pos: Position,
    pub message: String,
}

impl CronError {
    pub fn new(line: usize, col: usize, message: impl Into<String>) -> Self {
        CronError {
            pos: Position { line, col },
            message: message.into(),
        }
    }

    pub fn render(&self, source_line: &str, file_label: &str) -> String {
        let line_no = self.pos.line;
        let gutter = line_no.to_string().len();
        let pad = " ".repeat(gutter);
        let caret_offset = self.pos.col.saturating_sub(1);
        let caret_line = format!("{}^", " ".repeat(caret_offset));
        format!(
            "error: {message}\n  --> {file}:{line}:{col}\n{pad} |\n{line} | {source}\n{pad} | {caret}",
            message = self.message,
            file = file_label,
            line = line_no,
            col = self.pos.col,
            pad = pad,
            source = source_line,
            caret = caret_line,
        )
    }
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.pos.line, self.pos.col, self.message)
    }
}
