use crate::cron::AtomValue;
use crate::error::CronError;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuartzFieldKind {
    Second,
    Minute,
    Hour,
    DayOfMonth,
    Month,
    DayOfWeek,
    Year,
}

impl QuartzFieldKind {
    fn label(&self) -> &'static str {
        match self {
            QuartzFieldKind::Second => "second",
            QuartzFieldKind::Minute => "minute",
            QuartzFieldKind::Hour => "hour",
            QuartzFieldKind::DayOfMonth => "day-of-month",
            QuartzFieldKind::Month => "month",
            QuartzFieldKind::DayOfWeek => "day-of-week",
            QuartzFieldKind::Year => "year",
        }
    }

    fn bounds(&self) -> (u32, u32) {
        match self {
            QuartzFieldKind::Second => (0, 59),
            QuartzFieldKind::Minute => (0, 59),
            QuartzFieldKind::Hour => (0, 23),
            QuartzFieldKind::DayOfMonth => (1, 31),
            QuartzFieldKind::Month => (1, 12),
            // Quartz numbers Sunday as 1 and counts up, unlike standard
            // cron's 0-6 (with 7 as an alias for Sunday).
            QuartzFieldKind::DayOfWeek => (1, 7),
            // Matches the range Quartz's own CronExpression validates against.
            QuartzFieldKind::Year => (1970, 2199),
        }
    }

    fn names(&self) -> &'static [(&'static str, u32)] {
        match self {
            QuartzFieldKind::Month => &[
                ("JAN", 1), ("FEB", 2), ("MAR", 3), ("APR", 4),
                ("MAY", 5), ("JUN", 6), ("JUL", 7), ("AUG", 8),
                ("SEP", 9), ("OCT", 10), ("NOV", 11), ("DEC", 12),
            ],
            QuartzFieldKind::DayOfWeek => &[
                ("SUN", 1), ("MON", 2), ("TUE", 3), ("WED", 4),
                ("THU", 5), ("FRI", 6), ("SAT", 7),
            ],
            _ => &[],
        }
    }

    fn allows_question_mark(&self) -> bool {
        matches!(self, QuartzFieldKind::DayOfMonth | QuartzFieldKind::DayOfWeek)
    }
}

/// Mirrors `cron::Field`, but Quartz's day-of-month/day-of-week fields have a
/// third state ("?", meaning "no specific value") that plain cron has no
/// equivalent for.
#[derive(Debug, Clone)]
pub enum QuartzField {
    Any,
    Unspecified,
    List(Vec<AtomValue>),
}

impl QuartzField {
    pub fn is_any(&self) -> bool {
        matches!(self, QuartzField::Any)
    }

    pub fn is_unspecified(&self) -> bool {
        matches!(self, QuartzField::Unspecified)
    }
}

impl fmt::Display for QuartzField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuartzField::Any => write!(f, "*"),
            QuartzField::Unspecified => write!(f, "?"),
            QuartzField::List(atoms) => {
                for (i, atom) in atoms.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", atom)?;
                }
                Ok(())
            }
        }
    }
}

/// A parsed Quartz (6- or 7-field) cron line, plus the source positions a
/// conversion step needs to report its own errors.
pub struct QuartzSchedule {
    pub second: QuartzField,
    pub second_col: usize,
    pub minute: QuartzField,
    pub hour: QuartzField,
    pub day_of_month: QuartzField,
    pub day_of_month_col: usize,
    pub month: QuartzField,
    pub day_of_week: QuartzField,
    pub day_of_week_col: usize,
    pub year: QuartzField,
    pub year_col: usize,
    pub line: usize,
    pub command: String,
}

struct Token {
    text: String,
    col: usize,
}

// Splits a line into up to six whitespace-delimited fields, keeping the
// 1-based column each one starts at, plus whatever trails, unparsed, as
// `rest` (its own 1-based starting column included so callers can still
// report accurate error positions against it).
fn tokenize(line: &str) -> (Vec<Token>, usize, String) {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while tokens.len() < 6 && i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let text: String = chars[start..i].iter().collect();
        tokens.push(Token { text, col: start + 1 });
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let rest: String = chars[i..].iter().collect();
    (tokens, i + 1, rest)
}

// The optional 7th field and the command that may follow it share the same
// trailing text, and nothing marks where one ends and the other begins.
// We resolve that by trying to parse the first whitespace-delimited word
// of `rest` as a year field; if it parses, it's the year and whatever
// follows is the command, otherwise there's no year field and all of
// `rest` is the command. A command that happens to start with a bare
// number or a `*` will be misread as a year - there's no way around that
// ambiguity without a delimiter Quartz doesn't have.
fn split_year_and_command(rest: &str, rest_col: usize, line_no: usize) -> (QuartzField, usize, String) {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    if i == 0 {
        return (QuartzField::Any, rest_col, String::new());
    }
    let candidate: String = chars[..i].iter().collect();

    if let Ok(year) = parse_field(&candidate, rest_col, line_no, QuartzFieldKind::Year) {
        let command: String = chars[i..].iter().collect::<String>().trim().to_string();
        (year, rest_col, command)
    } else {
        (QuartzField::Any, rest_col, rest.trim_end().to_string())
    }
}

pub fn parse_line(line: &str, line_no: usize) -> Result<QuartzSchedule, CronError> {
    let (tokens, rest_col, rest) = tokenize(line);
    if tokens.len() < 6 {
        let col = line.chars().count() + 1;
        return Err(CronError::new(
            line_no,
            col.max(1),
            format!(
                "expected 6 fields (second minute hour day-of-month month day-of-week), plus an optional year, found {}",
                tokens.len()
            ),
        ));
    }

    let second = parse_field(&tokens[0].text, tokens[0].col, line_no, QuartzFieldKind::Second)?;
    let minute = parse_field(&tokens[1].text, tokens[1].col, line_no, QuartzFieldKind::Minute)?;
    let hour = parse_field(&tokens[2].text, tokens[2].col, line_no, QuartzFieldKind::Hour)?;
    let day_of_month = parse_field(&tokens[3].text, tokens[3].col, line_no, QuartzFieldKind::DayOfMonth)?;
    let month = parse_field(&tokens[4].text, tokens[4].col, line_no, QuartzFieldKind::Month)?;
    let day_of_week = parse_field(&tokens[5].text, tokens[5].col, line_no, QuartzFieldKind::DayOfWeek)?;
    let (year, year_col, command) = split_year_and_command(&rest, rest_col, line_no);

    Ok(QuartzSchedule {
        second,
        second_col: tokens[0].col,
        minute,
        hour,
        day_of_month,
        day_of_month_col: tokens[3].col,
        month,
        day_of_week,
        day_of_week_col: tokens[5].col,
        year,
        year_col,
        line: line_no,
        command,
    })
}

fn parse_field(text: &str, col: usize, line_no: usize, kind: QuartzFieldKind) -> Result<QuartzField, CronError> {
    if text == "*" {
        return Ok(QuartzField::Any);
    }
    if text == "?" {
        if !kind.allows_question_mark() {
            return Err(CronError::new(line_no, col, format!("'?' is not valid for {}", kind.label())));
        }
        return Ok(QuartzField::Unspecified);
    }

    let chars: Vec<char> = text.chars().collect();
    let mut atoms = Vec::new();
    let mut start = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c == ',' {
            let atom_text: String = chars[start..i].iter().collect();
            atoms.push(parse_atom(&atom_text, col + start, line_no, kind)?);
            start = i + 1;
        }
    }
    let atom_text: String = chars[start..].iter().collect();
    atoms.push(parse_atom(&atom_text, col + start, line_no, kind)?);

    Ok(QuartzField::List(atoms))
}

fn parse_atom(text: &str, col: usize, line_no: usize, kind: QuartzFieldKind) -> Result<AtomValue, CronError> {
    if text.is_empty() {
        return Err(CronError::new(line_no, col, "expected a value here, found nothing"));
    }

    if let Some(slash) = text.find('/') {
        let base_text = &text[..slash];
        let step_text = &text[slash + 1..];
        let step_col = col + slash + 1;
        if step_text.is_empty() || !step_text.chars().all(|c| c.is_ascii_digit()) {
            return Err(CronError::new(
                line_no,
                step_col,
                format!("step value '{}' must be a positive integer", step_text),
            ));
        }
        let step: u32 = step_text
            .parse()
            .map_err(|_| CronError::new(line_no, step_col, format!("step value '{}' is too large", step_text)))?;
        if step == 0 {
            return Err(CronError::new(line_no, step_col, "step value must be greater than zero"));
        }
        let base = if base_text == "*" {
            AtomValue::Any
        } else {
            parse_range_or_single(base_text, col, line_no, kind)?
        };
        return Ok(AtomValue::Step(Box::new(base), step));
    }

    parse_range_or_single(text, col, line_no, kind)
}

fn parse_range_or_single(text: &str, col: usize, line_no: usize, kind: QuartzFieldKind) -> Result<AtomValue, CronError> {
    if let Some(dash) = text.find('-') {
        let start_text = &text[..dash];
        let end_text = &text[dash + 1..];
        let end_col = col + dash + 1;
        let start_value = parse_single(start_text, col, line_no, kind)?;
        let end_value = parse_single(end_text, end_col, line_no, kind)?;
        if atom_numeric(&start_value) > atom_numeric(&end_value) {
            return Err(CronError::new(
                line_no,
                col,
                format!(
                    "range start ({}) is greater than range end ({})",
                    atom_numeric(&start_value),
                    atom_numeric(&end_value)
                ),
            ));
        }
        return Ok(AtomValue::Range(Box::new(start_value), Box::new(end_value)));
    }
    parse_single(text, col, line_no, kind)
}

fn parse_single(text: &str, col: usize, line_no: usize, kind: QuartzFieldKind) -> Result<AtomValue, CronError> {
    if text.is_empty() {
        return Err(CronError::new(line_no, col, "expected a value here, found nothing"));
    }

    if text.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = text
            .parse()
            .map_err(|_| CronError::new(line_no, col, format!("value '{}' is too large", text)))?;
        let (min, max) = kind.bounds();
        if n < min || n > max {
            return Err(CronError::new(
                line_no,
                col,
                format!("value {} is out of range for {} (expected {}-{})", n, kind.label(), min, max),
            ));
        }
        return Ok(AtomValue::Number(n));
    }

    let upper = text.to_ascii_uppercase();
    for entry in kind.names() {
        let (name, n) = *entry;
        if name == upper.as_str() {
            return Ok(AtomValue::Named(name.to_string(), n));
        }
    }

    Err(CronError::new(line_no, col, format!("invalid value '{}' for {}", text, kind.label())))
}

fn atom_numeric(atom: &AtomValue) -> u32 {
    match atom {
        AtomValue::Any => 0,
        AtomValue::Number(n) => *n,
        AtomValue::Named(_, n) => *n,
        AtomValue::Range(start, _) => atom_numeric(start),
        AtomValue::Step(base, _) => atom_numeric(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_six_fields_and_keeps_the_rest() {
        let (tokens, rest_col, rest) = tokenize("0 0 12 * * ? 2026 do the thing");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        let cols: Vec<usize> = tokens.iter().map(|t| t.col).collect();
        assert_eq!(texts, ["0", "0", "12", "*", "*", "?"]);
        assert_eq!(cols, [1, 3, 5, 7, 9, 11]);
        assert_eq!(rest_col, 13);
        assert_eq!(rest, "2026 do the thing");
    }

    #[test]
    fn tokenize_stops_at_six_fields_even_with_more_tokens() {
        let (tokens, _, rest) = tokenize("* * * * * * * *");
        assert_eq!(tokens.len(), 6);
        assert_eq!(rest, "* *");
    }

    #[test]
    fn split_year_and_command_finds_a_leading_year() {
        let (year, year_col, command) = split_year_and_command("2026 /usr/bin/backup.sh", 13, 1);
        assert_eq!(year.to_string(), "2026");
        assert_eq!(year_col, 13);
        assert_eq!(command, "/usr/bin/backup.sh");
    }

    #[test]
    fn split_year_and_command_treats_star_as_a_year_when_alone() {
        let (year, _, command) = split_year_and_command("* /usr/bin/backup.sh", 13, 1);
        assert!(year.is_any());
        assert_eq!(command, "/usr/bin/backup.sh");
    }

    #[test]
    fn split_year_and_command_falls_back_when_first_word_is_not_a_year() {
        let (year, _, command) = split_year_and_command("/usr/bin/backup.sh --now", 13, 1);
        assert!(year.is_any());
        assert_eq!(command, "/usr/bin/backup.sh --now");
    }

    #[test]
    fn split_year_and_command_handles_empty_rest() {
        let (year, year_col, command) = split_year_and_command("", 13, 1);
        assert!(year.is_any());
        assert_eq!(year_col, 13);
        assert_eq!(command, "");
    }

    #[test]
    fn split_year_and_command_rejects_out_of_range_year_as_a_year() {
        // 1969 fails the year bounds check, so it's read as a command instead.
        let (year, _, command) = split_year_and_command("1969 do the thing", 13, 1);
        assert!(year.is_any());
        assert_eq!(command, "1969 do the thing");
    }

    #[test]
    fn parse_line_accepts_a_full_six_field_line() {
        let schedule = parse_line("0 0 12 * * ? /usr/bin/backup.sh", 1).unwrap();
        assert_eq!(schedule.second.to_string(), "0");
        assert_eq!(schedule.hour.to_string(), "12");
        assert!(schedule.day_of_month.is_any());
        assert!(schedule.day_of_week.is_unspecified());
        assert!(schedule.year.is_any());
        assert_eq!(schedule.command, "/usr/bin/backup.sh");
    }

    #[test]
    fn parse_line_accepts_an_optional_seventh_year_field() {
        let schedule = parse_line("0 0 12 * * ? 2026 /usr/bin/backup.sh", 1).unwrap();
        assert_eq!(schedule.year.to_string(), "2026");
        assert_eq!(schedule.command, "/usr/bin/backup.sh");
    }

    #[test]
    fn parse_line_reports_missing_fields_at_end_of_line() {
        let err = parse_line("0 0 12 * *", 1).unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert!(err.message.contains("found 5"));
    }

    #[test]
    fn parse_field_question_mark_allowed_for_day_of_month() {
        let field = parse_field("?", 1, 1, QuartzFieldKind::DayOfMonth).unwrap();
        assert!(field.is_unspecified());
    }

    #[test]
    fn parse_field_question_mark_allowed_for_day_of_week() {
        let field = parse_field("?", 1, 1, QuartzFieldKind::DayOfWeek).unwrap();
        assert!(field.is_unspecified());
    }

    #[test]
    fn parse_field_question_mark_rejected_elsewhere() {
        for kind in [
            QuartzFieldKind::Second,
            QuartzFieldKind::Minute,
            QuartzFieldKind::Hour,
            QuartzFieldKind::Month,
            QuartzFieldKind::Year,
        ] {
            let err = parse_field("?", 4, 1, kind).unwrap_err();
            assert_eq!(err.pos.col, 4);
            assert!(err.message.contains("is not valid for"));
        }
    }

    #[test]
    fn parse_field_day_of_week_names_start_at_sunday_one() {
        let field = parse_field("SUN", 1, 1, QuartzFieldKind::DayOfWeek).unwrap();
        assert_eq!(field.to_string(), "SUN");
        if let QuartzField::List(atoms) = field {
            assert_eq!(atom_numeric(&atoms[0]), 1);
        } else {
            panic!("expected a list");
        }
    }

    #[test]
    fn parse_field_day_of_week_rejects_zero() {
        let err = parse_field("0", 1, 1, QuartzFieldKind::DayOfWeek).unwrap_err();
        assert!(err.message.contains("out of range for day-of-week"));
    }

    #[test]
    fn parse_field_year_accepts_bounds() {
        assert!(parse_field("1970", 1, 1, QuartzFieldKind::Year).is_ok());
        assert!(parse_field("2199", 1, 1, QuartzFieldKind::Year).is_ok());
        assert!(parse_field("2200", 1, 1, QuartzFieldKind::Year).is_err());
    }
}
