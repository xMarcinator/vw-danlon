# vw-danlon

Turns your car trips (myVolkswagen) into:

- **Working-time record** (arbejdstidsregistrering): every day, start, end,
  hours, norm and difference.
- **Mileage logbook** (kørselsregnskab) for tax-free kørselsgodtgørelse.
- **Pay lines** (lønart and quantity) for your own records.
- **Danløn app checklist**: per day, the Komme/gå and Kørsel registrations
  to type into the Danløn app, with a tick-off.

The tool runs only on your machine and never talks to Danløn.

The norm (planned hours) comes from a Proton Calendar plus Danish public
holidays.

## Monthly routine

```sh
cp config.example.toml config.toml   # once, then edit

cargo run -- trips import ~/Downloads/export.xlsx              # see "Trips from Min Volkswagen"
#   or: cargo run -- trips sync                                # see "Trip sync from the account"
cargo run -- report    2026-10                                 # check first
cargo run -- checklist 2026-10                                 # see "Danløn app checklist"
cargo run -- vacation                                          # see "Vacation"
cargo run -- timesheet 2026-10 -o arbejdstid-2026-10.csv
cargo run -- logbook   2026-10 -o korebog-2026-10.csv          # --all: every trip with its rule
cargo run -- trip business "2026-10-06 10:00"                 # or private / reset: override the rules
cargo run -- fradrag   2026                                    # befordringsfradrag estimate (info only)
cargo run -- export    2026-10 -o danlon-2026-10.txt
```

1. Print `korebog-…csv`, check it, and sign it as employer. The signed
   control is a condition for the allowance being tax-free.
2. Use `danlon-…txt` to check the month's pay lines (lønart and quantity).
3. Keep `arbejdstid-…csv` and `korebog-…csv` for at least 5 years.

Without `-t`, the commands use the trips stored by `trips import` or
`trips sync`. Pass
`-t trips.csv` (or a `.json` or `.xlsx` export) to use a file instead. Keep
all trips for the year: the 20,000 km rate threshold is counted from
1 January.

## Trips from Min Volkswagen

1. In the Min Volkswagen app, open **Ture → Historik** and tap the share
   icon. The app gives an Excel file (.xlsx) with the trips of the chosen
   period.
2. Run `cargo run -- trips import export.xlsx`. It prints how many trips
   were added, how many were already imported, and the period covered.

The trips are stored in the database `vw-danlon.db` beside the config (with
the local state and a copy of the calendar). Overlapping exports
are merged: a trip with the same start time and start odometer is added
once, so exporting a bit more than needed is safe. Trips of 0 km (the
engine only turned on) are skipped and counted as `skipped (0 km)`. The
driver's name and licence plate in the export are not stored.

A `trips.json` and `state.json` from an earlier version are moved into the
database on the first run: once the data is stored and read back, the JSON
files are deleted; if anything fails, they are left as they are and no
database is created. A database that can't be read stops every command
with its path and is never overwritten.

`backup export <file>` writes the config, trips and state to one file for
the app on another device (Settings → Data → Import backup). The calendar
link and the device token are not in it.

The calendar link is fetched for every command, and the copy is kept in
the database: when fetching fails, the kept copy is used with a warning
saying when it was fetched, and `--offline` uses it without fetching.

The export often leaves out house numbers ("Lysbrohøjen, Lysbro"). An
address without a house number matches a known address on the same street
and postal code or town. For a sure match, optionally give the coordinates
of home and the workplace (as copied from a map):

```toml
[places]
home = "56.1890, 10.1820"
workplace = "56.1572, 10.2107"
radius_m = 150          # default
```

A trip ending within `radius_m` of one counts as arriving there.

## Trip sync from the account (optional)

Instead of exporting by hand, `trips sync` can fetch the same trips from
your own Min Volkswagen account. This access is **unofficial**: Connected
Cars, the service behind the app, offers official access only to business
partners, so it may stop working at any time. The export above stays the
fallback and keeps working; both feed the same stored trips. The sync only
reads; nothing is written to the account.

Connect once:

```sh
cargo run -- account connect
```

It asks for the account email and password (the password is not shown and
never stored), registers this computer as a device and keeps only the
device token, in the system keychain. On a machine without a keychain
service, keep the token in a file instead:

```toml
[connected_cars]
email = "you@example.com"              # offered as the default
token_file = "connected-cars-token"    # relative to the config; git-ignored
```

The file is readable only by you (mode 0600) and holds the account email,
so another account's token is never used. If the account has several cars,
set `vehicle = "AB12345"` (plate or vehicle id); without it the sync lists
the cars and stops.

Then sync:

```sh
cargo run -- trips sync                      # from the newest stored trip's day
cargo run -- trips sync --from 2026-10-01    # from a given day
```

Without `--from` it starts on the day of the newest stored trip (1 January
when nothing is stored yet) and runs to today. Trips are fetched month by
month and merged like an export: a trip with the same start time and start
odometer is stored once, so syncing trips already imported from an export
adds nothing. It prints how many trips were added, how many were already
present, how many came without position, and the period. Trips without an
address or coordinates are still stored; turn on position sharing in the
app to get them.

If the service answers 429 (too many requests), the sync waits as long as
it asks and retries once. Any other failure stops the sync with the HTTP
status: the months before are stored, the failed month and later ones are
not, and running `trips sync` again continues from there. A device token
the service no longer accepts (unused for a year, or deleted) asks you to
run `account connect` again.

```sh
cargo run -- account status       # connected or not, and the last sync
cargo run -- account check        # fetch 30 days without storing, to test access
cargo run -- account disconnect   # delete the stored device token
```

## Danløn app checklist

Hours and driving are registered by hand in the Danløn app (Komme/gå and
Kørsel). `checklist` shows, per day, exactly what to type:

```sh
cargo run -- checklist 2026-11
```

```text
2026-11-18  ons  [ ] to enter
    Komme/gå  08:30 to 15:30 · break 30m · 6h 30m
    Kørsel    Purpose: Kunde A · Hjem → Kunde A · Add return trip: on · 148,0 km
2026-11-19  tor  [x] entered
    Komme/gå  07:58 to 10:00 · break none · 2h 02m
    Komme/gå  11:15 to 16:00 · break 30m · 4h 15m
2026-11-21  lør  nothing to enter (weekend)
2026-11-25  ons  fix first: ankomst til … uden registreret afgang

Days entered: 1 of 18 · Trips entered: 0 of 3 · 1 to fix first
```

In the Danløn app, per day:

- **Komme/gå**: one registration per line. Type the time range and the
  break on the app's keypad; leave Description empty. Private time splits
  the day into several lines and is never registered; the break is only
  the lunch. Check the app's Total time against the last figure.
- **Kørsel**: one registration per day. Purpose as shown, then the route
  stops by the names the app knows (home and the workplace are named with
  `[checklist] home_name`/`workplace_name`, default "Hjem" and "Kontoret";
  customers by their calendar entry's title). Switch on **Add return trip**
  at the bottom of the form when the line says `on`; the route then lists
  only the way out. Leave Description empty, and check the app's distance
  against the km shown. A `check: no trip from … to …` line means a trip is
  missing between two stops.

Then tick the day off; it is stored in the database beside the config:

```sh
cargo run -- checklist mark 2026-11-18
cargo run -- checklist unmark 2026-11-18     # undo
```

Days with nothing to enter (weekends, holidays, vacation, days off) and days that need
their trips fixed first cannot be ticked; `mark` says why. `mark` takes
`-t` like `report`. The checklist never contacts Danløn and writes nothing
but the tick in the database.

## Vacation

Register vacation in the calendar, as all-day events; there is nothing to
register in the tool itself.

- An all-day event whose title contains a word from `[vacation]` `keywords`
  (default `Ferie`) is vacation. The word may be part of a longer one, in any
  case: "Juleferie" and "Efterårsferie" count.
- A title containing a word from `feriefridag_keywords` (default
  `Feriefridag`) is a feriefridag instead; it wins over `keywords`.
- Only the working days in the event count: days with planned hours that are
  not a weekend or a public holiday. A two-week event over Christmas counts
  only the working days in it. An event with no working days counts 0 and is
  listed with "no working days".
- Vacation days are days off with norm 0 and the reason `Ferie` (or
  `Feriefridag`). The checklist shows them as "nothing to enter"; vacation
  is not entered in the Danløn app.

`vacation` shows the balance under the Danish rules for samtidighedsferie
(ferieloven):

- Vacation is earned per holiday year (ferieår), 1 September to 31 August,
  at 2,08 days per month of employment. A partial month, from the employment
  start, earns 0,07 days per calendar day.
- Days earned in a month can be taken from the following month. Taking them
  earlier is forskudsferie, and the period says so.
- Each holiday year's days must be used by 31 December of the following
  year (ferieafholdelsesperioden), so in September to December two holiday
  years are open at once. Taken and planned days are drawn from the oldest
  holiday year first.
- Days neither taken nor planned by the use-by date are shown as expiring.
  They can only be transferred by agreement with the employer, which the
  tool does not track.
- Feriefridage (from the contract, `feriefridage`, default 5) are counted
  separately per holiday year.

```sh
cargo run -- vacation
```

```text
Vacation · employed since 1 May 2026 · 2,08 days a month

2025/26 · earned 8,32 · taken 5,00 · planned 0,00 · left 3,32 · use by 31 Dec 2026
2026/27 · earned 2,08 (24,96 by 31 Aug 2027) · taken 0,00 · planned 0,00 · left 24,96 · use by 31 Dec 2027

Periods (oldest days are used first)
  19 Oct 2026 – 23 Oct 2026 · Efterårsferie · 5 working days · taken · 5,00 from 2025/26 (3,32 left)

Feriefridage 2026/27 · taken 0 · planned 0 · 5 of 5 left

warning: 3,32 days from 2025/26 expire on 31 Dec 2026
```

Each open holiday year shows the days earned so far (and by its end), taken,
planned, left and the use-by date; each period shows its working days and
the holiday year it draws from. Set the start of employment so earning
starts there:

```toml
[vacation]
employment_start = 2026-05-01
feriefridage = 5
```

Without `employment_start`, earning counts from 1 September of each holiday
year and the overview says so. Half days, holiday pay and ferietillæg are
not handled.

## Danish rules applied

**Working time (arbejdstidsloven, since 1 July 2024).** Daily working time
must be recorded objectively and reliably, be accessible to the employee and
be kept for 5 years. `timesheet` writes every day of the month with start,
end, lunch break and hours.

**Hours to eIndkomst.** For full-time monthly salaried employees Skat and
Danmarks Statistik expect the *norm* to be reported (160,33 h/month for
37 h/week), not actual hours. Danløn already does this from its Normtimer
field, so the default is `hours = "none"`. Set `hours = "actual"` and
`hours_lonart` to also export actual hours.

**Kørselsgodtgørelse (bekendtgørelse 173/2000, Skattestyrelsen).**
- A trip is reimbursed only when it has date, from/to (mål og delmål),
  business purpose and km. Trips missing any of these are listed as
  warnings and left out.
- Calendar entries away from `workplace` are business visits. The day's
  first trip arriving at the entry's address and the last trip leaving it
  are business trips, with the entry's title as purpose. Trips in between,
  such as a lunch errand, are not. A purpose in the trips file always wins.
- Home ↔ fixed workplace is commuting (LL § 9 B) and is never reimbursed;
  it is listed by `logbook --all` as driving.
- Home ↔ a client counts on at most 60 days per client address within 12
  months (several trips a day are one day; only trips starting or ending at
  home count). From the 61st day those trips are private; workplace ↔ client
  stays business. The count starts over after 60 working days in a row at
  other workplaces. The trips and calendar of the year before are read for
  this.
- Driving without a business purpose (errands on the way home, weekend
  driving) is private. Client → shops → home gets only the direct client →
  home km: the shortest drive between the two recorded earlier; without
  one the trip is flagged so you can set it with `trip business|private`.
- `trip business|private <YYYY-MM-DD HH:MM>` sets a stored trip by hand (e.g.
  driving between workplaces), winning over the rules; `trip reset` undoes it.
- The low rate applies up to 20,000 km per calendar year, the high rate
  above that. A trip crossing the threshold is split. A year without rates
  is an error only when it has business km.
- The logbook includes driver name, address (`mileage.address`, else
  `home`), CPR (`mileage.cpr`, blank when not set), registration number,
  business km this year to date and a field for the employer's signature.
- `fradrag <year>` estimates the befordringsfradrag (LL § 9 C) for your own
  TastSelv entry (felt 417 / rubrik 51): (round trip − 24 km) per commuting
  day at the year's rates (2026 built in: 3,17 kr/km to 120 km, 1,59 above,
  3,51 above 120 km with `mileage.yderkommune = true`). The round trip is
  twice the median commuting trip; home-working days, days off and days with
  reimbursed km from home are left out. Nothing is sent to Skat.

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
- **Private appointments** are timed events whose title contains a word
  from `private_keywords` (default `Læge`, `Tandlæge`, `Fysioterapeut`) as a
  whole word, in any case; "Lægemiddelfirma" is not "Læge". They are not
  company time:
  - not a work address, no planned hours, and no address needed;
  - the time is left out of worked hours, while the norm stays, so the
    appointment shows as hours to make up;
  - the day's note shows it, e.g. `Læge 10:00–11:15 · private`;
  - trips to or from its address, or starting during it, are private and
    never in the mileage logbook or the Danløn export.

  An empty `private_keywords` list switches this off.
- **All-day events** are ferie and fridage. These days get norm 0, also when
  the title is a private word. A title with a word from `[vacation]`
  `keywords` or `feriefridag_keywords` makes them vacation (see "Vacation").
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

A day's worked time is computed in this order:
1. the periods at a work address, from the trips, and the time working from
   home (see below; overlaps count once);
2. minus private appointments and recorded private time (see below;
   overlapping appointments count once; one crossing midnight is split per
   date);
3. minus the lunch break where it overlaps what is left (see below);
4. rounding (`periods.round_to_minutes`).

### Lunch

Each day has one lunch window, taken from the first of:
1. an override set with `lunch set <YYYY-MM-DD> <HH:MM> <minutes>` (removed
   again with `lunch clear <YYYY-MM-DD>`), kept in the database beside the
   config file; 0 minutes means no lunch that day;
2. a timed calendar event whose title contains `lunch.title` (default
   `Frokost`) as a whole word. It is not a work address and adds no planned
   hours;
3. the default, `[lunch]` `start` and `minutes` (12:00, 30 minutes).

Only the part of the window that overlaps time at work is deducted. Work
that ends before lunch, a lunch errand that already leaves a gap in the
periods, or a doctor's visit covering lunch deducts nothing more. The report
and the working-time record show each day's window and its source
(`default`, `calendar` or `override`).

The old `periods.unpaid_break_minutes` key is no longer accepted; use
`[lunch]` instead.

### Private time

For private time that is not in the calendar or takes longer than planned
(a doctor's visit with travel and waiting), press start when you leave and
stop when you are back:

```sh
cargo run -- private start [label]   # links the current or next private event
cargo run -- private stop
cargo run -- private list 2026-11
cargo run -- private edit <id> 12:25 14:10 [--label <label>]
cargo run -- private rm <id>
cargo run -- private start --at 12:50   # the real start of an appointment started automatically
cargo run -- private skip [--event <title>] [--date YYYY-MM-DD]   # you did not go
```

A private calendar appointment starts automatically: once its planned start
has passed and nothing records it, `private list` (and the app) show it as
"started automatically" from the planned start. `private start --at HH:MM`
sets its real start, `private stop` records its planned start until now, and
`private skip` says you did not go. Left untouched until `grace_minutes`
(default 60) after its planned end, its planned window is recorded and
marked "calendar time" the next time the CLI or the app loads it (there is
no background service); the month's numbers are the same before and after.
`private list` shows each entry's kind: recorded, calendar time or started
automatically. Every entry can be edited, relabelled or deleted at any time;
deleting one for a calendar appointment means you did not go, so that
appointment then counts no private time and is not recorded again. Switch
this off with `[private] auto_start = false`.

`private start` links to the private calendar appointment that is running,
or starts within 2 hours, that day, and takes its title as the label
(otherwise `Privat`). `--event <title>` links another appointment that day;
`--no-event` links none. Only one timer runs at a time.

Recorded intervals are kept in the database and count exactly like private
appointments: they are deducted from worked time, shown in the day's note
(e.g. `Privat 10:02–11:14 · private`), and trips starting during them are
private. A linked interval replaces the appointment's planned window, so
`Læge 13:00–14:00` measured as 12:30–14:10 deducts 1:40, once.

### Working from home

A timed work event in the calendar is home working when its title has a word
from `[home_working]` `keywords` (whole word, case-insensitive; default
`Hjemmearbejde`, `Hjemme`) or, with `at_home_address = true` (the default),
its location is the home address (`mileage.home`). Its planned window counts
as worked, minus lunch and private time, without any trips; on a day after
today it counts nothing yet. Home working that is not in the calendar is
timed like private time:

```sh
cargo run -- home start [label]   # label defaults to "Hjemmearbejde"
cargo run -- home stop
cargo run -- home list 2026-11
cargo run -- home edit <id> 08:00 12:30
cargo run -- home rm <id>
```

Recorded home working counts as worked and is shown in the day's note (e.g.
`Hjemmearbejde 08:00–12:30 · home`); the Danløn checklist gets Komme/gå
entries for it. A home-working timer still running past midnight or for more
than 12 hours is flagged in the report like the private timer.

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

A day without a work event, home working or norm hours (e.g. the weekend) is
never flagged: driving then is private.

The report's totals (norm, worked, difference) count the days up to and
including today, which is marked `◀ today`; later days show their planned
norm only. The month's whole norm is printed as `Norm hours in month`.

The report also flags a private timer still running past midnight or for more
than 12 hours; stop it and fix the time with `private edit`.

Public holidays are added automatically and always override the calendar.
Store bededag counts only before 2024. Grundlovsdag, 24/12 and 31/12 can each
be switched on or off.

## Trips file

Instead of an export, `-t` takes a CSV or JSON file with the fields:

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

## Before first use

- Set `employee_number` to your number in Danløn.
- Check the lønart codes in Danløn (mileage defaults to 39
  Kilometerpenge).
- Check the `[danlon]` delimiter, decimal format and column order against
  Danløn's current file description. Danløn changed its format in 2026.
- Check the year's km rates on skat.dk.

## Android phone app

The phone app is sideloaded as a signed release APK (arm64); it is not in a
store.

### Prerequisites

- [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started/) `dx`
  0.7.10 (`cargo install dioxus-cli --version 0.7.10`) and the Rust target
  `rustup target add aarch64-linux-android`.
- [Android SDK](https://developer.android.com/studio#command-line-tools-only)
  in `~/Android/Sdk` with build-tools (`zipalign`, `apksigner`, `aapt2`) and
  the [NDK](https://developer.android.com/ndk/downloads); the script picks
  the newest of each unless `ANDROID_HOME` / `ANDROID_NDK_HOME` are set.
- JDK 21 (`JAVA_HOME`, default `/usr/lib/jvm/java-21-openjdk`) for Gradle
  and `keytool`.
- [adb](https://developer.android.com/tools/adb) (SDK platform-tools).

### Release keystore (once)

The APK is signed with a local keystore that never goes into the repository:
`~/.config/vw-danlon/release.jks`, with its password in
`~/.config/vw-danlon/release.jks.pass` (mode 0600). To create it on a new
machine:

```sh
mkdir -p ~/.config/vw-danlon && chmod 700 ~/.config/vw-danlon
cd ~/.config/vw-danlon
(umask 077; head -c 32 /dev/urandom | base64 | tr -d '/+=' | head -c 32 > release.jks.pass)
KP=$(cat release.jks.pass) keytool -genkeypair -keystore release.jks \
  -storetype PKCS12 -alias vw-danlon -keyalg RSA -keysize 4096 \
  -validity 10000 -dname "CN=vw-danlon" -storepass:env KP -keypass:env KP
chmod 600 release.jks
```

Back up both files. Android only installs an update over the app when it is
signed with the same key: if the keystore is lost, uninstall the app (its
data on the phone goes with it) before installing a build signed with a new
keystore.

### Build and install

```sh
scripts/build-apk.sh             # -> dist/vw-danlon-<version>-arm64.apk
adb install -r dist/vw-danlon-0.2.0-arm64.apk
```

The script runs `dx bundle --platform android --release`, rebuilds the
generated Gradle project's release variant with the native library
stripped, then `zipalign`s, signs with the keystore and checks the result
with `apksigner verify`. `VW_DANLON_KEYSTORE` / `VW_DANLON_KEYSTORE_PASS`
point it at another keystore. `dist/` is git-ignored.

On the phone:

- **USB (adb):** enable Developer options (Settings → About phone → tap
  *Build number* seven times), turn on *USB debugging*, connect, accept the
  computer's key, then run `adb install -r` as above.
- **Without adb:** copy the APK to the phone, open it from the Files app and
  allow *Install unknown apps* for that app when asked.

`-r` keeps the app's data when updating.

## Code layout

A Cargo workspace. The root package is the `vw_danlon` library (the rules,
and `service`: loading the calendar and trips, building a month, its
mileage, checklist and vacation overview, the account sync) plus the
`vw-danlon` CLI in `src/main.rs`, which only parses arguments, prompts and
prints. `app/` is the desktop and phone app (`vw-danlon-app`).

```sh
cargo run -- report 2026-10      # the CLI, from the repository root
cargo test --workspace
```
