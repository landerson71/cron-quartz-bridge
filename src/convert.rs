use crate::cron::{AtomValue, Field, StandardSchedule};
use crate::error::CronError;
use crate::quartz::{QuartzField, QuartzSchedule};

/// Standard cron numbers Sunday as both 0 and 7; Quartz numbers it as 1 and
/// counts forward from there, so every other weekday shifts up by one.
fn shift_number(standard: u32) -> u32 {
    if standard == 0 || standard == 7 {
        1
    } else {
        standard + 1
    }
}

fn contains_number(atom: &AtomValue) -> bool {
    match atom {
        AtomValue::Any | AtomValue::Named(_, _) => false,
        AtomValue::Number(_) => true,
        AtomValue::Range(start, end) => contains_number(start) || contains_number(end),
        AtomValue::Step(base, _) => contains_number(base),
    }
}

// The set of raw (unshifted) cron day-of-week values a range or step atom
// expands to. A step's base anchors where the sequence starts: a bare value
// (or "*") runs to the field's upper bound (7, the alias for Sunday), while
// a range base is bounded on both ends by the range itself.
fn cron_dow_raw_values(atom: &AtomValue) -> Vec<u32> {
    match atom {
        AtomValue::Number(n) => vec![*n],
        AtomValue::Named(_, n) => vec![*n],
        AtomValue::Range(start, end) => (start.numeric()..=end.numeric()).collect(),
        AtomValue::Step(base, step) => {
            let (lo, hi) = match base.as_ref() {
                AtomValue::Any => (0, 7),
                AtomValue::Range(start, end) => (start.numeric(), end.numeric()),
                other => (other.numeric(), 7),
            };
            let mut values = Vec::new();
            let mut cur = lo;
            while cur <= hi {
                values.push(cur);
                cur += step;
            }
            values
        }
        AtomValue::Any => (0..=7).collect(),
    }
}

// Expands a range or step atom to its explicit set of matching days and
// shifts each one individually, rather than shifting only the endpoints.
// That distinction matters: cron's day-of-week wraps through 7 as an alias
// for Sunday, so a range like "5-7" (Fri-Sat-Sun) or a step like "1/2"
// (which lands on 7) would produce a nonsensical or out-of-order endpoint
// if only the boundary numbers were shifted.
fn shift_dow_values(atom: &AtomValue) -> Vec<u32> {
    let mut values: Vec<u32> = cron_dow_raw_values(atom).into_iter().map(shift_number).collect();
    values.sort_unstable();
    values.dedup();
    values
}

// Renders a sorted, deduped set of field values as compactly as possible:
// a single number, a contiguous range, or (when neither applies) an
// explicit comma-separated list.
fn format_number_set(values: &[u32]) -> String {
    match values {
        [] => String::new(),
        [single] => single.to_string(),
        _ if values.windows(2).all(|w| w[1] == w[0] + 1) => {
            format!("{}-{}", values[0], values[values.len() - 1])
        }
        _ => values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","),
    }
}

fn shift_atom(atom: &AtomValue) -> String {
    // Names mean the same weekday in both dialects, so a purely named atom
    // (e.g. "MON-FRI") needs no numeric translation at all.
    if !contains_number(atom) {
        return atom.to_string();
    }

    match atom {
        AtomValue::Number(n) => shift_number(*n).to_string(),
        AtomValue::Any | AtomValue::Named(_, _) => atom.to_string(),
        AtomValue::Range(_, _) | AtomValue::Step(_, _) => format_number_set(&shift_dow_values(atom)),
    }
}

fn shift_day_of_week(field: &Field) -> String {
    match field {
        Field::Any => "*".to_string(),
        Field::List(atoms) => atoms.iter().map(shift_atom).collect::<Vec<_>>().join(","),
    }
}

/// Converts a standard 5-field crontab line into a 6-field Quartz cron
/// expression (seconds prepended, `?` used for the unused day field).
pub fn to_quartz(schedule: &StandardSchedule) -> Result<String, CronError> {
    let dom_is_any = schedule.day_of_month.is_any();
    let dow_is_any = schedule.day_of_week.is_any();

    let (day_of_month_out, day_of_week_out) = if dom_is_any && dow_is_any {
        ("*".to_string(), "?".to_string())
    } else if dom_is_any {
        ("?".to_string(), shift_day_of_week(&schedule.day_of_week))
    } else if dow_is_any {
        (schedule.day_of_month.to_string(), "?".to_string())
    } else {
        return Err(CronError::new(
            schedule.line,
            schedule.day_of_month_col,
            "quartz has no way to express a schedule that fires on either a day-of-month or a day-of-week match; split this into two schedules",
        ));
    };

    let mut result = format!(
        "0 {} {} {} {} {}",
        schedule.minute, schedule.hour, day_of_month_out, schedule.month, day_of_week_out
    );
    if !schedule.command.is_empty() {
        result.push(' ');
        result.push_str(&schedule.command);
    }
    Ok(result)
}

fn unshift_number(quartz: u32) -> u32 {
    quartz - 1
}

// Quartz's day-of-week has no alias like cron's 0/7-for-Sunday, so unlike
// the forward direction there's no wraparound to worry about here - but a
// step's base still needs the same "runs to the field's upper bound"
// handling as the forward direction.
fn quartz_dow_raw_values(atom: &AtomValue) -> Vec<u32> {
    match atom {
        AtomValue::Number(n) => vec![*n],
        AtomValue::Named(_, n) => vec![*n],
        AtomValue::Range(start, end) => (start.numeric()..=end.numeric()).collect(),
        AtomValue::Step(base, step) => {
            let (lo, hi) = match base.as_ref() {
                AtomValue::Any => (1, 7),
                AtomValue::Range(start, end) => (start.numeric(), end.numeric()),
                other => (other.numeric(), 7),
            };
            let mut values = Vec::new();
            let mut cur = lo;
            while cur <= hi {
                values.push(cur);
                cur += step;
            }
            values
        }
        AtomValue::Any => (1..=7).collect(),
    }
}

fn unshift_dow_values(atom: &AtomValue) -> Vec<u32> {
    let mut values: Vec<u32> = quartz_dow_raw_values(atom).into_iter().map(unshift_number).collect();
    values.sort_unstable();
    values.dedup();
    values
}

fn unshift_atom(atom: &AtomValue) -> String {
    if !contains_number(atom) {
        return atom.to_string();
    }

    match atom {
        AtomValue::Number(n) => unshift_number(*n).to_string(),
        AtomValue::Any | AtomValue::Named(_, _) => atom.to_string(),
        AtomValue::Range(_, _) | AtomValue::Step(_, _) => format_number_set(&unshift_dow_values(atom)),
    }
}

fn unshift_day_of_week(field: &QuartzField) -> String {
    match field {
        QuartzField::Any | QuartzField::Unspecified => "*".to_string(),
        QuartzField::List(atoms) => atoms.iter().map(unshift_atom).collect::<Vec<_>>().join(","),
    }
}

fn validate_seconds(field: &QuartzField, line: usize, col: usize) -> Result<(), CronError> {
    if let QuartzField::List(atoms) = field {
        if let [AtomValue::Number(0)] = atoms.as_slice() {
            return Ok(());
        }
    }
    Err(CronError::new(
        line,
        col,
        "standard cron has no seconds field; the seconds value must be 0 to convert",
    ))
}

fn validate_year(field: &QuartzField, line: usize, col: usize) -> Result<(), CronError> {
    if field.is_any() {
        return Ok(());
    }
    Err(CronError::new(
        line,
        col,
        "standard cron has no year field; the year value must be * (or omitted) to convert",
    ))
}

/// Converts a Quartz (6- or 7-field) cron expression into a 5-field standard
/// crontab line.
pub fn to_standard(schedule: &QuartzSchedule) -> Result<String, CronError> {
    validate_seconds(&schedule.second, schedule.line, schedule.second_col)?;
    validate_year(&schedule.year, schedule.line, schedule.year_col)?;

    let dom_is_unspecified = schedule.day_of_month.is_unspecified();
    let dow_is_unspecified = schedule.day_of_week.is_unspecified();

    if dom_is_unspecified && dow_is_unspecified {
        return Err(CronError::new(
            schedule.line,
            schedule.day_of_month_col,
            "quartz requires exactly one of day-of-month or day-of-week to be specified; both are '?'",
        ));
    }
    if !dom_is_unspecified && !dow_is_unspecified {
        return Err(CronError::new(
            schedule.line,
            schedule.day_of_month_col,
            "quartz requires exactly one of day-of-month or day-of-week to be '?'",
        ));
    }

    let (day_of_month_out, day_of_week_out) = if dom_is_unspecified {
        let dow_out = if schedule.day_of_week.is_any() {
            "*".to_string()
        } else {
            unshift_day_of_week(&schedule.day_of_week)
        };
        ("*".to_string(), dow_out)
    } else {
        (schedule.day_of_month.to_string(), "*".to_string())
    };

    let mut result = format!(
        "{} {} {} {} {}",
        schedule.minute, schedule.hour, day_of_month_out, schedule.month, day_of_week_out
    );
    if !schedule.command.is_empty() {
        result.push(' ');
        result.push_str(&schedule.command);
    }
    Ok(result)
}
