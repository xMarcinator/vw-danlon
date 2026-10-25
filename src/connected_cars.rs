//! Read-only access to the Min Volkswagen account through the Connected Cars
//! backend: email/password login once to register a device, then device-token
//! logins and GraphQL trip queries. The access is unofficial, so every
//! unexpected answer is reported plainly. The password and access tokens are
//! never stored or printed; only the device token is kept, in the operating
//! system's keychain.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::trips::{ExportTrip, Position, Trip};

const AUTH_URL: &str = "https://auth-api.connectedcars.io";
const GRAPHQL_URL: &str = "https://api.connectedcars.io/graphql";
const KEYCHAIN_SERVICE: &str = "vw-danlon";
const DEVICE_NAME: &str = "vw-danlon";
const DEVICE_MODEL: &str = "vw-danlon command line";
/// Most trips one query asks for. A range that fills it is refused rather
/// than silently cut short.
const TRIPS_LIMIT: usize = 1000;

/// A non-success HTTP answer, kept typed so callers can act on the status
/// (e.g. a 429 with its Retry-After).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    /// Seconds from the Retry-After header, when given.
    pub retry_after: Option<u64>,
    pub message: String,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Connected Cars answered HTTP {}", self.status)?;
        if !self.message.is_empty() {
            write!(f, ": {}", self.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for HttpError {}

/// One HTTP answer, before interpretation.
struct Answer {
    status: u16,
    retry_after: Option<u64>,
    body: String,
}

impl Answer {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn http_error(&self) -> HttpError {
        HttpError {
            status: self.status,
            retry_after: self.retry_after,
            message: service_message(&self.body),
        }
    }
}

/// The `message` (else `error`) of an error body, or "" when it has none.
fn service_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    ["message", "error"]
        .iter()
        .find_map(|k| v.get(k).and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A logged-in session: the namespace and a short-lived access token.
pub struct Session {
    agent: ureq::Agent,
    namespace: String,
    access_token: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("vw-danlon/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn post(
    agent: &ureq::Agent,
    url: &str,
    namespace: &str,
    bearer: Option<&str>,
    body: &Value,
) -> Result<Answer> {
    let mut req = agent
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("X-Organization-Namespace", namespace);
    if let Some(token) = bearer {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let mut resp = req
        .send(body.to_string())
        .context("contacting Connected Cars")?;
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body = resp
        .body_mut()
        .read_to_string()
        .context("reading the Connected Cars response")?;
    Ok(Answer {
        status,
        retry_after,
        body,
    })
}

#[derive(Deserialize)]
struct TokenAnswer {
    token: String,
}

/// The access token from an email/password login answer.
fn password_login_token(a: &Answer) -> Result<String> {
    if !a.ok() {
        let msg = service_message(&a.body);
        let msg = if msg.is_empty() {
            format!("HTTP {}", a.status)
        } else {
            msg
        };
        bail!("login refused: {msg}");
    }
    access_token(a)
}

/// The access token from a device-token login answer. A refused device
/// token (expired, deleted or revoked) asks for a new `account connect`.
fn device_login_token(a: &Answer) -> Result<String> {
    if matches!(a.status, 400 | 401 | 403 | 404) {
        bail!(
            "the stored device token was refused (expired or deleted): reconnect with `account connect`"
        );
    }
    if !a.ok() {
        return Err(a.http_error().into());
    }
    access_token(a)
}

fn access_token(a: &Answer) -> Result<String> {
    serde_json::from_str::<TokenAnswer>(&a.body)
        .map(|t| t.token)
        .map_err(|_| anyhow!("unexpected login answer from Connected Cars (no token)"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAnswer {
    device_token: String,
}

fn registered_device_token(a: &Answer) -> Result<String> {
    if !a.ok() {
        return Err(a.http_error()).context("registering this device");
    }
    serde_json::from_str::<DeviceAnswer>(&a.body)
        .map(|d| d.device_token)
        .map_err(|_| anyhow!("unexpected answer registering this device (no device token)"))
}

/// Logs in with email and password and registers this computer as a
/// device. Returns the device token; the password and the access token are
/// dropped.
pub fn register_device(namespace: &str, email: &str, password: &str) -> Result<String> {
    let agent = agent();
    let login = post(
        &agent,
        &format!("{AUTH_URL}/auth/login/email/password"),
        namespace,
        None,
        &json!({ "email": email, "password": password }),
    )?;
    let access = password_login_token(&login)?;
    let device = post(
        &agent,
        &format!("{AUTH_URL}/user/registerDevice"),
        namespace,
        Some(&access),
        &json!({ "deviceName": DEVICE_NAME, "deviceModel": DEVICE_MODEL }),
    )?;
    registered_device_token(&device)
}

impl Session {
    /// Logs in with a stored device token.
    pub fn login(namespace: &str, device_token: &str) -> Result<Session> {
        let agent = agent();
        let answer = post(
            &agent,
            &format!("{AUTH_URL}/auth/login/deviceToken"),
            namespace,
            None,
            &json!({ "deviceToken": device_token }),
        )?;
        let access_token = device_login_token(&answer)?;
        Ok(Session {
            agent,
            namespace: namespace.to_string(),
            access_token,
        })
    }

    fn query(&self, query: &str) -> Result<Value> {
        let answer = post(
            &self.agent,
            GRAPHQL_URL,
            &self.namespace,
            Some(&self.access_token),
            &json!({ "query": query }),
        )?;
        graphql_data(&answer)
    }

    /// The vehicles on the account.
    pub fn vehicles(&self) -> Result<Vec<Vehicle>> {
        parse_vehicles(&self.query(VEHICLES_QUERY)?)
    }

    /// The vehicle's completed trips that end on or after `from` and start
    /// on or before `to`, oldest first.
    pub fn trips(
        &self,
        vehicle_id: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        tz: Tz,
    ) -> Result<Vec<ExportTrip>> {
        parse_trips(&self.query(&trips_query(vehicle_id, from, to)?)?, tz)
    }
}

/// The `data` of a GraphQL answer; HTTP and GraphQL errors become errors.
fn graphql_data(a: &Answer) -> Result<Value> {
    if a.status == 401 {
        bail!(
            "Connected Cars refused the access token (HTTP 401): reconnect with `account connect`"
        );
    }
    if !a.ok() {
        return Err(a.http_error().into());
    }
    let v: Value = serde_json::from_str(&a.body)
        .map_err(|_| anyhow!("unexpected answer from Connected Cars (not JSON)"))?;
    if let Some(errors) = v
        .get("errors")
        .and_then(Value::as_array)
        .filter(|e| !e.is_empty())
    {
        let msgs: Vec<&str> = errors
            .iter()
            .map(|e| {
                e.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
            })
            .collect();
        bail!("Connected Cars query failed: {}", msgs.join("; "));
    }
    match v.get("data") {
        Some(d) if !d.is_null() => Ok(d.clone()),
        _ => bail!("unexpected answer from Connected Cars (no data)"),
    }
}

const VEHICLES_QUERY: &str =
    "query Vehicles { viewer { vehicles { vehicle { id licensePlate } } } }";

/// A vehicle on the account. Shown to pick one; never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vehicle {
    pub id: String,
    pub plate: String,
}

fn parse_vehicles(data: &Value) -> Result<Vec<Vehicle>> {
    let list = data
        .pointer("/viewer/vehicles")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("unexpected answer from Connected Cars (no vehicle list)"))?;
    list.iter()
        .map(|item| {
            let v = item.get("vehicle").unwrap_or(&Value::Null);
            let id = match v.get("id") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => bail!("unexpected answer from Connected Cars (vehicle without id)"),
            };
            let plate = v
                .get("licensePlate")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(Vehicle { id, plate })
        })
        .collect()
}

/// The vehicle the config names (plate or id), or the only one.
pub fn pick_vehicle<'a>(vehicles: &'a [Vehicle], wanted: Option<&str>) -> Result<&'a Vehicle> {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect::<String>()
    };
    let list = || {
        vehicles
            .iter()
            .map(|v| format!("{} (id {})", v.plate, v.id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (wanted, vehicles) {
        (_, []) => bail!("the account has no vehicles"),
        (Some(w), _) => vehicles
            .iter()
            .find(|v| v.id == w.trim() || (!v.plate.is_empty() && norm(&v.plate) == norm(w)))
            .ok_or_else(|| anyhow!("no vehicle {w:?} on the account; it has {}", list())),
        (None, [only]) => Ok(only),
        (None, _) => bail!(
            "the account has several vehicles: {}; set connected_cars.vehicle to the plate or id",
            list()
        ),
    }
}

const TRIP_FIELDS: &str = "startTime endTime startAddressString endAddressString \
     startLatitude startLongitude endLatitude endLongitude \
     startOdometer endOdometer mileage duration tripType note";

fn trips_query(vehicle_id: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<String> {
    if vehicle_id.is_empty() || !vehicle_id.chars().all(|c| c.is_ascii_digit()) {
        bail!("vehicle id {vehicle_id:?} is not a number");
    }
    let time =
        |t: DateTime<Utc>| format!("{:?}", t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    Ok(format!(
        "query Trips {{ vehicle(id: {vehicle_id}) {{ trips(fromTime: {}, toTime: {}, last: {TRIPS_LIMIT}, ignoreEmpty: true) {{ items {{ {TRIP_FIELDS} }} }} }} }}",
        time(from),
        time(to)
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTrip {
    start_time: Option<String>,
    end_time: Option<String>,
    start_address_string: Option<String>,
    end_address_string: Option<String>,
    start_latitude: Option<f64>,
    start_longitude: Option<f64>,
    end_latitude: Option<f64>,
    end_longitude: Option<f64>,
    start_odometer: Option<f64>,
    end_odometer: Option<f64>,
    mileage: Option<f64>,
}

/// Maps a trips answer to export trips, oldest first. A trip without an end
/// time (still driving) is left out; trip type and note are not read.
fn parse_trips(data: &Value, tz: Tz) -> Result<Vec<ExportTrip>> {
    let items = data
        .pointer("/vehicle/trips/items")
        .ok_or_else(|| anyhow!("unexpected answer from Connected Cars (no trips)"))?;
    let raw: Vec<RawTrip> = serde_json::from_value(items.clone())
        .context("unexpected trip fields from Connected Cars")?;
    if raw.len() >= TRIPS_LIMIT {
        bail!("{TRIPS_LIMIT} or more trips in one query; use a shorter period");
    }
    let time = |s: &str| {
        DateTime::parse_from_rfc3339(s)
            .map(|t| t.with_timezone(&tz))
            .map_err(|_| anyhow!("unrecognised trip time {s:?} from Connected Cars"))
    };
    let pos = |lat: Option<f64>, lon: Option<f64>| {
        Some(Position {
            lat: lat?,
            lon: lon?,
        })
    };
    let text = |s: Option<String>| s.map(|s| s.trim().to_string()).unwrap_or_default();
    let mut trips = Vec::new();
    for r in raw {
        let (Some(start), Some(end)) = (r.start_time.as_deref(), r.end_time.as_deref()) else {
            continue;
        };
        let (start, end) = (time(start)?, time(end)?);
        if end < start {
            bail!("trip from Connected Cars ends before it starts: {start} → {end}");
        }
        let distance_km = match (r.start_odometer, r.end_odometer) {
            (Some(s), Some(e)) if e >= s => Some(e - s),
            _ => r.mileage,
        };
        trips.push(ExportTrip {
            trip: Trip {
                start,
                end,
                distance_km,
                from: text(r.start_address_string),
                to: text(r.end_address_string),
                purpose: String::new(),
                business: None,
                start_pos: pos(r.start_latitude, r.start_longitude),
                end_pos: pos(r.end_latitude, r.end_longitude),
            },
            start_odometer: r.start_odometer,
        });
    }
    trips.sort_by_key(|t| t.trip.start);
    Ok(trips)
}

/// Trips that came without coordinates (position sharing off in the app).
pub fn without_position(trips: &[ExportTrip]) -> usize {
    trips
        .iter()
        .filter(|t| t.trip.start_pos.is_none() || t.trip.end_pos.is_none())
        .count()
}

/// Where the device token is kept: the operating system's keychain, or,
/// when configured with `token_file`, a file only the user can read (for
/// machines without a keychain service).
pub enum TokenStore {
    Keychain,
    File(PathBuf),
}

/// The token file's content: the token belongs to one account.
#[derive(Serialize, Deserialize)]
struct TokenFile {
    email: String,
    device_token: String,
}

impl TokenStore {
    pub fn describe(&self) -> String {
        match self {
            TokenStore::Keychain => "the keychain".into(),
            TokenStore::File(p) => p.display().to_string(),
        }
    }

    /// The stored device token for `email`, if any.
    pub fn get(&self, email: &str) -> Result<Option<String>> {
        match self {
            TokenStore::Keychain => match keychain_entry(email)?.get_password() {
                Ok(t) => Ok(Some(t)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(keychain_error(e)),
            },
            TokenStore::File(p) => {
                let text = match std::fs::read_to_string(p) {
                    Ok(t) => t,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(e).with_context(|| format!("reading {}", p.display())),
                };
                let f: TokenFile = serde_json::from_str(&text)
                    .with_context(|| format!("parsing token file {}", p.display()))?;
                Ok((f.email == email).then_some(f.device_token))
            }
        }
    }

    /// Fails unless the store can be read, so nothing is registered that
    /// cannot be stored.
    pub fn check(&self, email: &str) -> Result<()> {
        self.get(email).map(|_| ())
    }

    pub fn set(&self, email: &str, token: &str) -> Result<()> {
        match self {
            TokenStore::Keychain => keychain_entry(email)?
                .set_password(token)
                .map_err(keychain_error),
            TokenStore::File(p) => write_private(
                p,
                &TokenFile {
                    email: email.into(),
                    device_token: token.into(),
                },
            )
            .with_context(|| format!("writing token file {}", p.display())),
        }
    }

    /// Deletes the stored device token; false when there was none.
    pub fn delete(&self, email: &str) -> Result<bool> {
        match self {
            TokenStore::Keychain => match keychain_entry(email)?.delete_credential() {
                Ok(()) => Ok(true),
                Err(keyring::Error::NoEntry) => Ok(false),
                Err(e) => Err(keychain_error(e)),
            },
            TokenStore::File(p) => {
                if self.get(email)?.is_none() {
                    return Ok(false);
                }
                std::fs::remove_file(p).with_context(|| format!("deleting {}", p.display()))?;
                Ok(true)
            }
        }
    }
}

/// The keychain entry holding the device token for `email`.
fn keychain_entry(email: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, email).map_err(keychain_error)
}

fn keychain_error(e: keyring::Error) -> anyhow::Error {
    anyhow!(
        "no keychain available: {e} (set connected_cars.token_file to keep the token in a file)"
    )
}

/// Writes `value` as JSON readable only by the user (mode 0600 on Unix),
/// through a temp file and a rename.
fn write_private(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    serde_json::to_writer_pretty(&mut f, value)?;
    std::io::Write::write_all(&mut f, b"\n")?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::Europe::Copenhagen;

    fn answer(status: u16, body: &str) -> Answer {
        Answer {
            status,
            retry_after: None,
            body: body.into(),
        }
    }

    fn data(body: &str) -> Value {
        graphql_data(&answer(200, body)).unwrap()
    }

    #[test]
    fn trips_map_with_addresses_coordinates_and_odometer_km() {
        let body = r#"{"data":{"vehicle":{"trips":{"items":[
            {"startTime":"2026-10-06T05:40:00.000Z","endTime":"2026-10-06T06:05:30.000Z",
             "startAddressString":"Hjemvej 3, 8200 Aarhus N","endAddressString":" Kontorvej 1, 8000 Aarhus C ",
             "startLatitude":56.189,"startLongitude":10.182,"endLatitude":56.1572,"endLongitude":10.2107,
             "startOdometer":12000.5,"endOdometer":12014.0,"mileage":13.9,"duration":1530,
             "tripType":null,"note":null},
            {"startTime":"2026-10-05T14:00:00.000Z","endTime":"2026-10-05T14:20:00.000Z",
             "startAddressString":"A","endAddressString":"B",
             "startLatitude":56.0,"startLongitude":10.0,"endLatitude":56.1,"endLongitude":10.1,
             "startOdometer":null,"endOdometer":null,"mileage":7.25,"duration":1200,
             "tripType":"business","note":"x"},
            {"startTime":"2026-10-06T08:00:00.000Z","endTime":null,"mileage":1.0}
        ]}}}}"#;
        let trips = parse_trips(&data(body), Copenhagen).unwrap();
        assert_eq!(trips.len(), 2, "the trip still driving is left out");
        let t = &trips[1];
        assert_eq!(
            t.trip.start,
            Copenhagen.with_ymd_and_hms(2026, 10, 6, 7, 40, 0).unwrap()
        );
        assert_eq!(
            t.trip.end,
            Copenhagen.with_ymd_and_hms(2026, 10, 6, 8, 5, 30).unwrap()
        );
        assert_eq!(t.trip.from, "Hjemvej 3, 8200 Aarhus N");
        assert_eq!(t.trip.to, "Kontorvej 1, 8000 Aarhus C");
        assert_eq!(
            t.trip.distance_km,
            Some(13.5),
            "odometer difference wins over mileage"
        );
        assert_eq!(t.start_odometer, Some(12000.5));
        assert_eq!(
            t.trip.start_pos,
            Some(Position {
                lat: 56.189,
                lon: 10.182
            })
        );
        assert_eq!(
            t.trip.end_pos,
            Some(Position {
                lat: 56.1572,
                lon: 10.2107
            })
        );
        assert_eq!(
            trips[0].trip.distance_km,
            Some(7.25),
            "mileage without odometer"
        );
        assert_eq!(trips[0].start_odometer, None);
        assert_eq!(without_position(&trips), 0);
    }

    #[test]
    fn trip_without_addresses_or_coordinates_still_maps() {
        let body = r#"{"data":{"vehicle":{"trips":{"items":[
            {"startTime":"2026-10-07T05:40:00Z","endTime":"2026-10-07T06:00:00Z",
             "startAddressString":null,"endAddressString":null,
             "startLatitude":null,"startLongitude":null,"endLatitude":null,"endLongitude":null,
             "startOdometer":100.0,"endOdometer":110.0,"mileage":null}
        ]}}}}"#;
        let trips = parse_trips(&data(body), Copenhagen).unwrap();
        assert_eq!(trips.len(), 1);
        let t = &trips[0].trip;
        assert_eq!((t.from.as_str(), t.to.as_str()), ("", ""));
        assert_eq!((t.start_pos, t.end_pos), (None, None));
        assert_eq!(t.distance_km, Some(10.0));
        assert_eq!(without_position(&trips), 1);
    }

    #[test]
    fn errors_map_to_the_spec_messages() {
        let refused = password_login_token(&answer(
            401,
            r#"{"error":"Unauthorized","message":"Wrong email or password"}"#,
        ))
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "login refused: Wrong email or password"
        );
        let bare = password_login_token(&answer(500, "")).unwrap_err();
        assert_eq!(bare.to_string(), "login refused: HTTP 500");

        for status in [400, 401, 403, 404] {
            let e = device_login_token(&answer(status, r#"{"error":"x"}"#)).unwrap_err();
            assert!(
                e.to_string().contains("reconnect with `account connect`"),
                "{status}: {e}"
            );
        }
        let busy = device_login_token(&Answer {
            status: 429,
            retry_after: Some(30),
            body: r#"{"message":"slow down"}"#.into(),
        })
        .unwrap_err();
        assert_eq!(
            busy.downcast_ref::<HttpError>(),
            Some(&HttpError {
                status: 429,
                retry_after: Some(30),
                message: "slow down".into()
            })
        );

        let expired = graphql_data(&answer(401, "")).unwrap_err();
        assert!(
            expired
                .to_string()
                .contains("reconnect with `account connect`")
        );
        let gql = graphql_data(&answer(
            200,
            r#"{"data":null,"errors":[{"message":"Cannot query field \"x\""}]}"#,
        ))
        .unwrap_err();
        assert_eq!(
            gql.to_string(),
            r#"Connected Cars query failed: Cannot query field "x""#
        );

        assert_eq!(
            device_login_token(&answer(
                200,
                r#"{"token":"jwt","level":"AM1","expires":3600}"#
            ))
            .unwrap(),
            "jwt"
        );
        assert_eq!(
            registered_device_token(&answer(
                200,
                r#"{"deviceName":"n","deviceModel":"m","deviceToken":"dt"}"#
            ))
            .unwrap(),
            "dt"
        );
    }

    #[test]
    fn vehicle_is_picked_by_plate_or_id() {
        let vehicles = parse_vehicles(&data(
            r#"{"data":{"viewer":{"vehicles":[
                {"primary":true,"vehicle":{"id":"123","licensePlate":"AB 12 345"}},
                {"primary":false,"vehicle":{"id":456,"licensePlate":"CD67890"}}]}}}"#,
        ))
        .unwrap();
        assert_eq!(pick_vehicle(&vehicles, Some("ab12345")).unwrap().id, "123");
        assert_eq!(
            pick_vehicle(&vehicles, Some("456")).unwrap().plate,
            "CD67890"
        );
        assert!(
            pick_vehicle(&vehicles, None)
                .unwrap_err()
                .to_string()
                .contains("several vehicles: AB 12 345 (id 123), CD67890 (id 456)")
        );
        assert!(pick_vehicle(&vehicles, Some("ZZ")).is_err());
        assert_eq!(pick_vehicle(&vehicles[..1], None).unwrap().id, "123");
    }

    #[test]
    fn trips_query_quotes_times_and_refuses_odd_ids() {
        let from = Utc.with_ymd_and_hms(2026, 9, 30, 22, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 10, 31, 23, 0, 0).unwrap();
        let q = trips_query("123", from, to).unwrap();
        assert!(q.contains(r#"vehicle(id: 123) { trips(fromTime: "2026-09-30T22:00:00Z", toTime: "2026-10-31T23:00:00Z""#), "{q}");
        assert!(trips_query("1) { x", from, to).is_err());
    }

    #[test]
    fn token_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("vwd-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = TokenStore::File(dir.join("token"));
        assert_eq!(store.get("a@example.com").unwrap(), None);
        store.set("a@example.com", "dev-123").unwrap();
        assert_eq!(
            store.get("a@example.com").unwrap().as_deref(),
            Some("dev-123")
        );
        // Another account's token is not handed out.
        assert_eq!(store.get("b@example.com").unwrap(), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("token"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!store.delete("b@example.com").unwrap());
        assert!(store.delete("a@example.com").unwrap());
        assert_eq!(store.get("a@example.com").unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
