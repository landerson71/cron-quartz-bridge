mod convert;
mod cron;
mod error;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return ExitCode::SUCCESS;
    }
    if args.len() > 2 {
        print_usage();
        return ExitCode::from(2);
    }

    let (source, file_label) = match args.get(1) {
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

    let mut had_error = false;
    for (i, line) in source.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let result = cron::parse_line(line, line_no).and_then(|schedule| convert::to_quartz(&schedule));
        match result {
            Ok(quartz) => println!("{}", quartz),
            Err(err) => {
                had_error = true;
                eprintln!("{}", err.render(line, &file_label));
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
    eprintln!("usage: cronvert [FILE]");
    eprintln!();
    eprintln!("Reads standard 5-field crontab lines from FILE (or stdin if omitted)");
    eprintln!("and prints the equivalent Quartz cron expression for each line.");
}
