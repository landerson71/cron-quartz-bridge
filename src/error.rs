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

    pub fn render(&self, source_line: &str, file_label: &str, color: bool) -> String {
        let line_no = self.pos.line;
        let gutter = line_no.to_string().len();
        let pad = " ".repeat(gutter);
        let caret_offset = self.pos.col.saturating_sub(1);

        let paint = |code: &str, text: &str| -> String {
            if color {
                format!("\x1b[{}m{}\x1b[0m", code, text)
            } else {
                text.to_string()
            }
        };

        let error_label = paint("1;31", "error");
        let message = paint("1", &self.message);
        let arrow = paint("1;34", "-->");
        let bar = paint("1;34", "|");
        let line_no_painted = paint("1;34", &line_no.to_string());
        let caret = paint("1;31", &format!("{}^", " ".repeat(caret_offset)));

        format!(
            "{error_label}: {message}\n{pad}{arrow} {file}:{line}:{col}\n{pad} {bar}\n{line_no} {bar} {source}\n{pad} {bar} {caret}",
            file = file_label,
            line = line_no,
            col = self.pos.col,
            pad = pad,
            source = source_line,
            line_no = line_no_painted,
        )
    }
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.pos.line, self.pos.col, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_without_color_has_no_escape_codes() {
        let err = CronError::new(1, 4, "value 60 is out of range for minute");
        let rendered = err.render("60 9 * * 1 backup.sh", "<stdin>", false);
        assert!(!rendered.contains('\x1b'));
        assert!(rendered.contains("error: value 60 is out of range for minute"));
        assert!(rendered.contains("--> <stdin>:1:4"));
    }

    #[test]
    fn render_with_color_wraps_pieces_in_escape_codes_and_resets() {
        let err = CronError::new(2, 1, "boom");
        let rendered = err.render("* * * * *", "file.txt", true);
        assert!(rendered.contains("\x1b[1;31merror\x1b[0m"));
        assert!(rendered.contains("\x1b[1;34m-->\x1b[0m"));
        // stripping every escape sequence should leave exactly the plain
        // rendering, so color never changes the visible text.
        let stripped = strip_ansi(&rendered);
        let plain = err.render("* * * * *", "file.txt", false);
        assert_eq!(stripped, plain);
    }

    #[test]
    fn caret_lines_up_under_the_reported_column() {
        let err = CronError::new(1, 4, "bad");
        let rendered = err.render("abc def", "f", false);
        let caret_line = rendered.lines().last().unwrap();
        // "1 | " gutter prefix is 4 columns wide, then 3 spaces to reach col 4.
        assert_eq!(caret_line, "  |    ^");
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }
}
