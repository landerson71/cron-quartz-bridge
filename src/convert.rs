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

#[cfg(test)]
mod to_quartz_tests {
    use super::*;
    use crate::cron;

    fn quartz_dow_of(cron_line: &str) -> String {
        let schedule = cron::parse_line(cron_line, 1).unwrap();
        let out = to_quartz(&schedule).unwrap();
        out.split_whitespace().nth(5).unwrap().to_string()
    }

    #[test]
    fn shift_number_maps_both_sunday_aliases_to_one() {
        assert_eq!(shift_number(0), 1);
        assert_eq!(shift_number(7), 1);
    }

    #[test]
    fn shift_number_shifts_other_weekdays_up_by_one() {
        assert_eq!(shift_number(1), 2);
        assert_eq!(shift_number(6), 7);
    }

    #[test]
    fn to_quartz_shifts_a_bare_sunday_number() {
        assert_eq!(quartz_dow_of("0 9 * * 0 /usr/bin/backup.sh"), "1");
    }

    #[test]
    fn to_quartz_shifts_the_sunday_alias_seven() {
        assert_eq!(quartz_dow_of("0 9 * * 7 /usr/bin/backup.sh"), "1");
    }

    #[test]
    fn to_quartz_named_weekday_range_is_unchanged() {
        assert_eq!(quartz_dow_of("0 9 * * MON-FRI /usr/bin/backup.sh"), "MON-FRI");
    }

    #[test]
    fn to_quartz_shifts_a_range_that_wraps_through_the_sunday_alias() {
        // Cron's 5-7 is Fri-Sat-Sun; shifted that's 6, 7, 1, which isn't
        // contiguous once sorted, so it must render as an explicit list.
        assert_eq!(quartz_dow_of("0 9 * * 5-7 /usr/bin/backup.sh"), "1,6,7");
    }

    #[test]
    fn to_quartz_shifts_a_step_that_lands_on_the_sunday_alias() {
        // Cron's 1/2 (starting Mon, every other day to the field's upper
        // bound of 7) expands to 1,3,5,7; shifted that's 2,4,6,1.
        assert_eq!(quartz_dow_of("0 9 * * 1/2 /usr/bin/backup.sh"), "1,2,4,6");
    }

    #[test]
    fn to_quartz_shifts_a_step_with_an_any_base() {
        // "*/3" bases at cron's lower bound 0 and steps to 7: 0,3,6 -> 1,4,7.
        assert_eq!(quartz_dow_of("0 9 * * */3 /usr/bin/backup.sh"), "1,4,7");
    }

    #[test]
    fn to_quartz_shifts_each_entry_of_a_comma_list_independently() {
        // Each comma-separated atom is shifted on its own, so 0 and 7 (both
        // aliasing Sunday) each become 1 rather than collapsing into one.
        assert_eq!(quartz_dow_of("0 9 * * 0,1,7 /usr/bin/backup.sh"), "1,2,1");
    }

    #[test]
    fn to_quartz_rejects_both_day_fields_specified() {
        let schedule = cron::parse_line("0 9 15 * 1 /usr/bin/backup.sh", 1).unwrap();
        let err = to_quartz(&schedule).unwrap_err();
        assert!(err.message.contains("either a day-of-month or a day-of-week"));
    }
}

#[cfg(test)]
mod to_standard_tests {
    use super::*;
    use crate::cron;
    use crate::quartz;

    fn standard_dow_of(quartz_line: &str) -> String {
        let schedule = quartz::parse_line(quartz_line, 1).unwrap();
        let out = to_standard(&schedule).unwrap();
        out.split_whitespace().nth(4).unwrap().to_string()
    }

    #[test]
    fn unshift_number_reverses_shift_number_for_named_sunday() {
        assert_eq!(unshift_number(1), 0);
        assert_eq!(unshift_number(7), 6);
    }

    #[test]
    fn to_standard_unshifts_a_bare_sunday_number() {
        assert_eq!(standard_dow_of("0 0 9 ? * 1 /usr/bin/backup.sh"), "0");
    }

    #[test]
    fn to_standard_named_weekday_range_is_unchanged() {
        assert_eq!(standard_dow_of("0 0 9 ? * MON-FRI /usr/bin/backup.sh"), "MON-FRI");
    }

    #[test]
    fn to_standard_unshifts_a_range_spanning_past_sunday() {
        // Quartz's 5-7 (Thu-Fri-Sat) unshifts to 4-6.
        assert_eq!(standard_dow_of("0 0 9 ? * 5-7 /usr/bin/backup.sh"), "4-6");
    }

    #[test]
    fn to_standard_unshifts_a_step_with_an_any_base() {
        // "*/3" in Quartz bases at 1 and steps to 7: 1,4,7 -> unshifted 0,3,6.
        assert_eq!(standard_dow_of("0 0 9 ? * */3 /usr/bin/backup.sh"), "0,3,6");
    }

    #[test]
    fn round_trips_a_shifted_weekday_through_both_directions() {
        let forward = cron::parse_line("0 9 * * 5 /usr/bin/backup.sh", 1).unwrap();
        let quartz_line = to_quartz(&forward).unwrap();
        let back = quartz::parse_line(&quartz_line, 1).unwrap();
        let standard_line = to_standard(&back).unwrap();
        assert_eq!(standard_line, "0 9 * * 5 /usr/bin/backup.sh");
    }
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
