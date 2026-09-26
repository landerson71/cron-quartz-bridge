mod convert;
mod cron;
mod error;
mod quartz;

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return ExitCode::SUCCESS;
    }

    let mut reverse = false;
    let mut no_color = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in args.iter().skip(1) {
        if arg == "-r" || arg == "--reverse" {
            reverse = true;
        } else if arg == "--no-color" {
            no_color = true;
        } else {
            positional.push(arg.clone());
        }
    }
    if positional.len() > 1 {
        print_usage();
        return ExitCode::from(2);
    }

    let (source, file_label) = match positional.first() {
        Some(path) => match fs::read_to_string(path) {
            Ok(contents) => (contents, path.clone()),
            Err(e) => {
                eprintln!("error: could not read '{}': {}", path, e);
                return ExitCode::FAILURE;
            }
        },
        None => {
            let mut buf = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut buf) {
                eprintln!("error: could not read stdin: {}", e);
                return ExitCode::FAILURE;
            }
            (buf, "<stdin>".to_string())
        }
    };

    // Respect the NO_COLOR convention (https://no-color.org) and skip escape
    // codes entirely when stderr isn't a terminal, e.g. when piped to a file.
    // --no-color is an explicit override for cases where the environment
    // can't be changed, e.g. a CI runner that sets its own terminal vars.
    let color = !no_color && env::var_os("NO_COLOR").is_none() && io::stderr().is_terminal();

    let mut had_error = false;
    for (i, line) in source.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let result = if reverse {
            quartz::parse_line(line, line_no).and_then(|schedule| convert::to_standard(&schedule))
        } else {
            cron::parse_line(line, line_no).and_then(|schedule| convert::to_quartz(&schedule))
        };
        match result {
            Ok(converted) => println!("{}", converted),
            Err(err) => {
                had_error = true;
                eprintln!("{}", err.render(line, &file_label, color));
            }
        }
    }

    if had_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_usage() {
    eprintln!("usage: cronvert [-r|--reverse] [--no-color] [FILE]");
    eprintln!();
    eprintln!("Reads standard 5-field crontab lines from FILE (or stdin if omitted)");
    eprintln!("and prints the equivalent Quartz cron expression for each line.");
    eprintln!();
    eprintln!("With -r/--reverse, reads 6-field Quartz cron expressions instead and");
    eprintln!("prints the equivalent standard crontab line for each one.");
    eprintln!();
    eprintln!("--no-color disables colored error output regardless of NO_COLOR or");
    eprintln!("whether stderr is a terminal.");
}
