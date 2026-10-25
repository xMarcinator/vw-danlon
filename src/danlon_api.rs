//! Sends pay lines to Danløn through its GraphQL API (api.danlon.dk).
//!
//! Authentication uses a customer refresh token from Danløn, exchanged for a
//! 5-minute access token on each run. The API allows no concurrent requests
//! and at most 30 per minute; a run makes a handful.
//!
//! Units, rate and amount are integers with two implied decimals, as in
//! Danløn's file import: 196,50 km is sent as 19650.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::config::ApiConfig;
use crate::danlon::Line;

pub const TOKEN_ENV: &str = "DANLON_REFRESH_TOKEN";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub id: String,
    /// The employee number shown in Danløn.
    pub domain_id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayCode {
    pub code: String,
    pub description: String,
    pub units_allowed: bool,
    pub rate_allowed: bool,
    pub amount_allowed: bool,
}

#[derive(Debug, Deserialize)]
pub struct PayPart {
    pub id: String,
    pub code: String,
    pub units: Option<i64>,
    pub rate: Option<i64>,
    pub amount: Option<i64>,
}

/// A pay part to create, in hundredths.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPayPart {
    pub code: String,
    pub units: Option<i64>,
    pub rate: Option<i64>,
    pub amount: Option<i64>,
}

pub struct Client {
    agent: ureq::Agent,
    url: String,
    access_token: String,
}

impl Client {
    /// Exchanges the refresh token for an access token.
    pub fn connect(cfg: &ApiConfig) -> Result<Self> {
        let refresh_token = refresh_token(cfg.refresh_token_file.as_deref())?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(false)
            .build()
            .into();
        let mut client = Client {
            agent,
            url: cfg.url.clone(),
            access_token: String::new(),
        };
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Token {
            access_token: String,
        }
        let data: Value = client
            .request(
                "mutation ($t: String!) { token(input: { refreshToken: $t }) { accessToken } }",
                json!({ "t": refresh_token }),
            )
            .context("logging in to Danløn — check the refresh token")?;
        let token: Token = take(data, "token")?;
        client.access_token = token.access_token;
        Ok(client)
    }

    fn request(&self, query: &str, variables: Value) -> Result<Value> {
        let body = json!({ "query": query, "variables": variables }).to_string();
        let mut req = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json");
        if !self.access_token.is_empty() {
            req = req.header("Authorization", &format!("Bearer {}", self.access_token));
        }
        let mut resp = req.send(&body).context("calling the Danløn API")?;
        let status = resp.status();
        let text = resp
            .body_mut()
            .read_to_string()
            .context("reading the Danløn API response")?;
        let mut v: Value = serde_json::from_str(&text)
            .with_context(|| format!("Danløn API returned HTTP {status}: {text}"))?;
        if let Some(errors) = v.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            let msgs: Vec<_> = errors
                .iter()
                .map(|e| e["message"].as_str().unwrap_or("unknown error"))
                .collect();
            bail!("Danløn API: {}", msgs.join("; "));
        }
        if !status.is_success() {
            bail!("Danløn API returned HTTP {status}: {text}");
        }
        Ok(v["data"].take())
    }

    /// The company the refresh token was issued for.
    pub fn current_company(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        let data = self.request("{ currentCompany { id } }", json!({}))?;
        Ok(take::<Id>(data, "currentCompany")?.id)
    }

    pub fn employees(&self, company_id: &str) -> Result<Vec<Employee>> {
        let data = self.request(
            "query ($c: ID!) { companiesExt(input: { companyIds: [$c] }) { companies {
                employees(input: { employeeIds: null }) { employees { id domainId name active } }
            } } }",
            json!({ "c": company_id }),
        )?;
        let company = data["companiesExt"]["companies"]
            .get(0)
            .ok_or_else(|| anyhow!("Danløn returned no company {company_id}"))?;
        serde_json::from_value(company["employees"]["employees"].clone())
            .context("reading employees")
    }

    pub fn pay_codes(&self) -> Result<Vec<PayCode>> {
        let data = self.request(
            "{ payPartsMeta { payPartsMeta { code description unitsAllowed rateAllowed amountAllowed } } }",
            json!({}),
        )?;
        let meta: Value = take(data, "payPartsMeta")?;
        serde_json::from_value(meta["payPartsMeta"].clone()).context("reading pay codes")
    }

    /// Pay parts waiting for the next payroll.
    pub fn pay_parts(&self, company_id: &str, employee_id: &str) -> Result<Vec<PayPart>> {
        let data = self.request(
            "query ($c: ID!, $e: ID) { payParts(input: { companyId: $c, employeeId: $e }) {
                payParts { id code units rate amount } } }",
            json!({ "c": company_id, "e": employee_id }),
        )?;
        let parts: Value = take(data, "payParts")?;
        serde_json::from_value(parts["payParts"].clone()).context("reading pay parts")
    }

    pub fn create(
        &self,
        company_id: &str,
        employee_id: &str,
        parts: &[NewPayPart],
    ) -> Result<Vec<PayPart>> {
        let parts: Vec<Value> = parts
            .iter()
            .map(|p| {
                json!({
                    "employeeId": employee_id,
                    "code": p.code,
                    "units": p.units,
                    "rate": p.rate,
                    "amount": p.amount,
                })
            })
            .collect();
        let data = self.request(
            "mutation ($c: ID!, $p: [CreatePayPartExtInput!]!) {
                createPayParts(input: { companyId: $c, payParts: $p }) {
                    createdPayParts { id code units rate amount } } }",
            json!({ "c": company_id, "p": parts }),
        )?;
        let created: Value = take(data, "createPayParts")?;
        serde_json::from_value(created["createdPayParts"].clone())
            .context("reading created pay parts")
    }

    pub fn delete(&self, company_id: &str, ids: &[String]) -> Result<()> {
        self.request(
            "mutation ($c: ID!, $ids: [ID!]) {
                deletePayParts(input: { companyId: $c, payPartIds: $ids }) { __typename } }",
            json!({ "c": company_id, "ids": ids }),
        )?;
        Ok(())
    }
}

fn take<T: DeserializeOwned>(mut data: Value, field: &str) -> Result<T> {
    let v = data[field].take();
    if v.is_null() {
        bail!("Danløn API returned no {field}");
    }
    serde_json::from_value(v).with_context(|| format!("reading {field}"))
}

fn refresh_token(file: Option<&Path>) -> Result<String> {
    if let Ok(t) = std::env::var(TOKEN_ENV)
        && !t.trim().is_empty()
    {
        return Ok(t.trim().to_string());
    }
    let Some(file) = file else {
        bail!("no Danløn refresh token: set {TOKEN_ENV} or danlon.api.refresh_token_file");
    };
    let t = std::fs::read_to_string(file)
        .with_context(|| format!("reading refresh token {}", file.display()))?;
    Ok(t.trim().to_string())
}

/// The Danløn employee to send pay parts for: `employee_id` if set,
/// otherwise the employee whose number is `employee_number`.
pub fn find_employee<'a>(
    employees: &'a [Employee],
    employee_id: Option<&str>,
    employee_number: &str,
) -> Result<&'a Employee> {
    let found = match employee_id {
        Some(id) => employees.iter().find(|e| e.id == id),
        None => employees
            .iter()
            .find(|e| e.domain_id.trim() == employee_number.trim()),
    };
    found.ok_or_else(|| {
        anyhow!(
            "employee {} not found in Danløn — run `danlon info` and check employee_number",
            employee_id.unwrap_or(employee_number)
        )
    })
}

fn hundredths(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

/// Turns export lines into pay parts, sending only the fields Danløn allows
/// for each code. Lines whose code takes no rate (e.g. 39 Kilometerpenge,
/// where Danløn applies its own rate) are merged into one part per code.
pub fn to_pay_parts(lines: &[Line], codes: &[PayCode]) -> Result<Vec<NewPayPart>> {
    let mut out: Vec<NewPayPart> = Vec::new();
    for line in lines {
        let meta = codes
            .iter()
            .find(|c| c.code.eq_ignore_ascii_case(line.lonart.trim()))
            .ok_or_else(|| {
                anyhow!(
                    "lønart {:?} does not exist in Danløn — run `danlon info` for the codes",
                    line.lonart
                )
            })?;
        let units = meta.units_allowed.then_some(line.quantity);
        let amount = if meta.amount_allowed {
            line.amount()
        } else {
            None
        };
        if units.is_none() && amount.is_none() {
            bail!(
                "lønart {} ({}) takes neither units nor an amount for {}",
                meta.code,
                meta.description,
                line.text
            );
        }
        let rate = if meta.rate_allowed { line.rate } else { None };
        let part = NewPayPart {
            code: meta.code.clone(),
            units: units.map(hundredths),
            rate: rate.map(hundredths),
            amount: amount.map(hundredths),
        };
        let sum = |a: Option<i64>, b: Option<i64>| a.map(|a| a + b.unwrap_or(0));
        match out.iter_mut().find(|p| p.code == part.code) {
            Some(existing) if !meta.rate_allowed => {
                existing.units = sum(existing.units, part.units);
                existing.amount = sum(existing.amount, part.amount);
            }
            _ => out.push(part),
        }
    }
    Ok(out)
}

/// Shows a value in hundredths as a Danish decimal number.
pub fn show(v: Option<i64>) -> String {
    v.map(|v| format!("{:.2}", v as f64 / 100.0).replace('.', ","))
        .unwrap_or_else(|| "–".into())
}

/// Resolves a relative token file against the config file's directory.
pub fn resolve_token_file(file: &mut Option<PathBuf>, config_dir: &Path) {
    if let Some(f) = file
        && f.is_relative()
    {
        *f = config_dir.join(&*f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(code: &str, units: bool, rate: bool, amount: bool) -> PayCode {
        PayCode {
            code: code.into(),
            description: String::new(),
            units_allowed: units,
            rate_allowed: rate,
            amount_allowed: amount,
        }
    }

    fn line(lonart: &str, quantity: f64, rate: Option<f64>) -> Line {
        Line {
            lonart: lonart.into(),
            quantity,
            rate,
            text: "test",
        }
    }

    #[test]
    fn kilometerpenge_merged_without_rate() {
        let codes = [code("39", true, false, false)];
        let lines = [line("39", 196.5, Some(3.94)), line("39", 10.0, Some(2.28))];
        assert_eq!(
            to_pay_parts(&lines, &codes).unwrap(),
            [NewPayPart {
                code: "39".into(),
                units: Some(20650),
                rate: None,
                amount: None,
            }]
        );
    }

    #[test]
    fn rated_code_keeps_one_part_per_rate() {
        let codes = [code("43", true, true, true)];
        let lines = [line("43", 196.5, Some(3.94)), line("43", 10.0, Some(2.28))];
        let parts = to_pay_parts(&lines, &codes).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].units, Some(19650));
        assert_eq!(parts[0].rate, Some(394));
        assert_eq!(parts[0].amount, Some(77421));
    }

    #[test]
    fn unknown_code_is_an_error() {
        let codes = [code("39", true, false, false)];
        assert!(to_pay_parts(&[line("YYYY", 1.0, None)], &codes).is_err());
    }

    #[test]
    fn employee_by_number() {
        let employees = vec![Employee {
            id: "MV8xXzE=".into(),
            domain_id: "7".into(),
            name: "Test".into(),
            active: true,
        }];
        assert_eq!(find_employee(&employees, None, "7").unwrap().id, "MV8xXzE=");
        assert!(find_employee(&employees, None, "8").is_err());
    }
}
