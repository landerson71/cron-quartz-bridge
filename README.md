# cron-quartz-bridge

Converts standard crontab schedules (the 5-field `minute hour dom month dow`
format used by cron/Vixie cron) into Quartz cron expressions (the 6-field
format Quartz and Spring's `@Scheduled` use).

The two dialects look similar but disagree in ways that are easy to get
wrong by hand:

- Quartz has a leading seconds field that plain cron doesn't.
- Quartz requires that exactly one of day-of-month / day-of-week be `?`,
  because it treats the other as "don't care" rather than "any". Cron uses
  `*` for both and lets the scheduler figure it out.
- The two numbering schemes for day-of-week don't line up: cron treats
  both `0` and `7` as Sunday and counts up from there, while Quartz starts
  at `1` for Sunday.

`cronvert` reads crontab-style lines and prints the Quartz equivalent for
each one. When a line can't be converted, it says exactly where the problem
is, down to the column.

## Usage

```
$ cat crontab.txt
0 9 * * MON-FRI /usr/bin/backup.sh
*/15 * 1 * * echo hello
30 2 * * 0 /opt/scripts/weekly.sh

$ cronvert crontab.txt
0 0 9 ? * MON-FRI /usr/bin/backup.sh
0 */15 * 1 * ? echo hello
0 30 2 ? * 1 /opt/scripts/weekly.sh
```

Without a file argument it reads from stdin:

```
$ echo '*/5 * * * * /usr/bin/heartbeat' | cronvert
0 */5 * * * ? /usr/bin/heartbeat
```

Pass `-r`/`--reverse` to go the other way, reading Quartz 6-field
expressions and printing the standard crontab equivalent:

```
$ echo '0 0 9 ? * MON-FRI /usr/bin/backup.sh' | cronvert -r
0 9 * * MON-FRI /usr/bin/backup.sh
```

The seconds field has to be `0`, since standard cron has no sub-minute
resolution:

```
$ echo '30 0 9 ? * MON-FRI /usr/bin/backup.sh' | cronvert -r
error: standard cron has no seconds field; the seconds value must be 0 to convert
  --> <stdin>:1:1
  |
1 | 30 0 9 ? * MON-FRI /usr/bin/backup.sh
  | ^
```

Blank lines and lines starting with `#` are skipped, same as in a real
crontab.

## Error messages

Every field is validated against real cron bounds (minutes 0-59, hours
0-23, and so on), and a bad value is reported with a caret pointing at the
exact character:

```
$ echo '60 9 * * 1 /usr/bin/backup.sh' | cronvert
error: value 60 is out of range for minute (expected 0-59)
  --> <stdin>:1:1
  |
1 | 60 9 * * 1 /usr/bin/backup.sh
  | ^
```

The same applies to conversion problems that only show up once the two
fields interact, such as specifying both day-of-month and day-of-week
(something plain cron allows but Quartz has no way to express):

```
$ echo '0 9 15 * 1 /usr/bin/backup.sh' | cronvert
error: quartz has no way to express a schedule that fires on either a day-of-month or a day-of-week match; split this into two schedules
  --> <stdin>:1:9
  |
1 | 0 9 15 * 1 /usr/bin/backup.sh
  |         ^
```

## Current limitations

- Only the 6-field Quartz form is accepted and produced; the optional 7th
  (year) field isn't handled.

## Building

No third-party crates, so a plain `cargo build --release` produces
`target/release/cronvert`.

## License

MIT, see [LICENSE](LICENSE).
