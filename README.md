# vw-danlon

Turns your car trips (myVolkswagen) into:

- **Working-time record** (arbejdstidsregistrering): every day, start, end,
  hours, norm and difference.
- **Mileage logbook** (kørselsregnskab) for tax-free kørselsgodtgørelse.
- **Danløn import file** (Lønberegning → Indlæs fra fil).

The norm (planned hours) comes from a Proton Calendar plus Danish public
holidays.

## Monthly routine

```sh
cp config.example.toml config.toml   # once, then edit

cargo run -- report    2026-10 -t trips.csv                    # check first
cargo run -- timesheet 2026-10 -t trips.csv -o arbejdstid-2026-10.csv
cargo run -- logbook   2026-10 -t trips.csv -o korebog-2026-10.csv
cargo run -- export    2026-10 -t trips.csv -o danlon-2026-10.txt
```

1. Print `korebog-…csv`, check it, and sign it as employer. The signed
   control is a condition for the allowance being tax-free.
2. Import `danlon-…txt` in Danløn under **Lønberegning → Indlæs fra fil**,
   or send it directly with `export 2026-10 -t trips.csv --send` (see below).
3. Keep `arbejdstid-…csv` and `korebog-…csv` for at least 5 years.

Keep all trips for the year in one file. The 20,000 km rate threshold is
counted from 1 January.

## Danish rules applied

**Working time (arbejdstidsloven, since 1 July 2024).** Daily working time
must be recorded objectively and reliably, be accessible to the employee and
be kept for 5 years. `timesheet` writes every day of the month with start,
end, break and hours.

**Hours to eIndkomst.** For full-time monthly salaried employees Skat and
Danmarks Statistik expect the *norm* to be reported (160,33 h/month for
37 h/week), not actual hours. Danløn already does this from its Normtimer
field, so the default is `hours = "none"`. Set `hours = "actual"` and
`hours_lonart` to also send actual hours.

**Kørselsgodtgørelse (bekendtgørelse 173/2000, Skattestyrelsen).**
- A trip is reimbursed only when it has date, from/to (mål og delmål),
  business purpose and km. Trips missing any of these are listed as
  warnings and left out.
- Calendar entries away from `workplace` are business visits. The day's
  first trip arriving at the entry's address and the last trip leaving it
  are business trips, with the entry's title as purpose. Trips in between,
  such as a lunch errand, are not. A purpose in the trips file always wins.
- Home ↔ fixed workplace is commuting and is never reimbursed.
- The low rate applies up to 20,000 km per calendar year, the high rate
  above that. A trip crossing the threshold is split.
- The logbook includes driver name, registration number and a field for the
  employer's signature.

## Calendar (Proton)

Create a dedicated calendar, e.g. "Arbejdstid", and set `calendar.name` to
the same name. The program stops if the link points at another calendar.

- **Timed events** are your planned working hours, and each one must have
  the **work address in the location field**.
  - Use a repeating event for your normal week, e.g. Mon–Fri 08:00–15:24 at
    the office.
  - Edit a single occurrence, or add an event, for days at another address,
    e.g. a customer.
  - With `norm_from_calendar = true` the events set each day's norm.
- **All-day events** are ferie and fridage. These days get norm 0.
- `default_location` (optional) is used on days with no located entry.

Share it: Settings → All settings → Calendars → (calendar) → *Share with
anyone* → Create link, with full view. Put the link in `calendar.source`.
A downloaded `.ics` file also works.

## How working time is matched

Working time is the time spent at the day's work address. It runs from a
trip **arriving** at the calendar entry's address until a trip **departing**
from it. If you leave and come back during the day (lunch, an errand), you
get several periods. With `count_travel_time = true`, the drive there and
back is included.

The fixed `workplace` always counts as a work address, also on days where
the calendar says you are elsewhere. If a colleague drives you from the
office to a customer, the car only shows you arriving at and leaving the
office, and that is your working time. A drive from one work address to
another (office → customer) is working time too.

Addresses are compared loosely:
- case, punctuation and "Danmark" are ignored
- the same street and number with the same (or a missing) postal code counts
  as a match

So `Vestergade 12, 5000 Odense C, Danmark` matches
`Vestergade 12, 5000 Odense`.

Some days are flagged in the report and the working-time record for you to
check:
- an arrival with no departure
- driving on a day with no work address
- a calendar entry with no address

Public holidays are added automatically and always override the calendar.
Store bededag counts only before 2024. Grundlovsdag, 24/12 and 31/12 can each
be switched on or off.

## Trips file

CSV or JSON with the fields:

| field | required | example |
|---|---|---|
| `start`, `end` | yes | `2026-10-06 07:45` or RFC 3339 |
| `distance_km` | for mileage | `61.0` |
| `from`, `to` | for mileage | `Hjem`, `Kunde A Odense` |
| `purpose` | for mileage | `Servicebesøg` |
| `business` | no | `true`/`false`; if empty, a trip with a purpose is business |

`from`/`to` must be addresses, so they can be matched against the calendar
and against `mileage.home`/`mileage.workplace`. See `examples/trips.csv`.
Without a `[calendar]`, a day runs from the first departure to the last
arrival.

## Sending to Danløn (API)

`export --send` sends the pay lines through Danløn's GraphQL API
(api.danlon.dk) instead of, or as well as, writing the file. It shows what
will be sent, warns if lines with the same code are already waiting in
Danløn, and asks before sending.

```sh
export DANLON_REFRESH_TOKEN=…        # or danlon.api.refresh_token_file
cargo run -- danlon info              # company, employees, pay codes, waiting pay parts
cargo run -- export 2026-10 -t trips.csv --send
cargo run -- danlon delete <id>       # undo, before running payroll
```

- Get the refresh token from Danløn. Keep it out of git.
- The employee is found by `employee_number` (Danløn's medarbejdernummer).
- Only the fields Danløn allows for each code are sent. For 39
  Kilometerpenge that is the km; Danløn applies its own rate.
- Values are sent in hundredths, as in the file import (196,50 → 19650).
  Check the first upload in Danløn before running payroll.

## Before the first real import

- Set `employee_number` to your number in Danløn.
- Check the lønart codes with `danlon info` (mileage defaults to 39
  Kilometerpenge).
- Check the `[danlon]` delimiter, decimal format and column order against
  Danløn's current file description. Danløn changed its format in 2026.
- Check the year's km rates on skat.dk.
- Do a test import before running payroll.

## Not yet done

- Fetching trips directly from myVolkswagen. There is no official API, and
  the unofficial login used by CarConnectivity would have to be ported.
