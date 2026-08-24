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

fn shift_atom(atom: &AtomValue, line: usize, col: usize) -> Result<String, CronError> {
    // Names mean the same weekday in both dialects, so a purely named atom
    // (e.g. "MON-FRI") needs no numeric translation at all.
    if !contains_number(atom) {
        return Ok(atom.to_string());
    }

    match atom {
        AtomValue::Number(n) => Ok(shift_number(*n).to_string()),
        AtomValue::Any | AtomValue::Named(_, _) => Ok(atom.to_string()),
        AtomValue::Range(_, _) | AtomValue::Step(_, _) => Err(CronError::new(
            line,
            col,
            "a numeric day-of-week range or step can't be re-numbered automatically; rewrite it using weekday names (SUN-SAT) instead",
        )),
    }
}

fn shift_day_of_week(field: &Field, line: usize, col: usize) -> Result<String, CronError> {
    match field {
        Field::Any => Ok("*".to_string()),
        Field::List(atoms) => {
            let mut parts = Vec::with_capacity(atoms.len());
            for atom in atoms {
                parts.push(shift_atom(atom, line, col)?);
            }
            Ok(parts.join(","))
        }
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
        ("?".to_string(), shift_day_of_week(&schedule.day_of_week, schedule.line, schedule.day_of_week_col)?)
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

fn unshift_atom(atom: &AtomValue, line: usize, col: usize) -> Result<String, CronError> {
    if !contains_number(atom) {
        return Ok(atom.to_string());
    }

    match atom {
        AtomValue::Number(n) => Ok(unshift_number(*n).to_string()),
        AtomValue::Any | AtomValue::Named(_, _) => Ok(atom.to_string()),
        AtomValue::Range(_, _) | AtomValue::Step(_, _) => Err(CronError::new(
            line,
            col,
            "a numeric day-of-week range or step can't be re-numbered automatically; rewrite it using weekday names (SUN-SAT) instead",
        )),
    }
}

fn unshift_day_of_week(field: &QuartzField, line: usize, col: usize) -> Result<String, CronError> {
    match field {
        QuartzField::Any | QuartzField::Unspecified => Ok("*".to_string()),
        QuartzField::List(atoms) => {
            let mut parts = Vec::with_capacity(atoms.len());
            for atom in atoms {
                parts.push(unshift_atom(atom, line, col)?);
            }
            Ok(parts.join(","))
        }
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

/// Converts a Quartz (6-field) cron expression into a 5-field standard
/// crontab line.
pub fn to_standard(schedule: &QuartzSchedule) -> Result<String, CronError> {
    validate_seconds(&schedule.second, schedule.line, schedule.second_col)?;

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
            unshift_day_of_week(&schedule.day_of_week, schedule.line, schedule.day_of_week_col)?
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
