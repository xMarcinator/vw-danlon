//! vw-danlon: working time, mileage, vacation and the Danløn app checklist
//! from a work calendar and myVolkswagen trips. Used by the `vw-danlon` CLI
//! and the app; [`service`] holds the steps both run to produce a month's
//! data, the other modules the rules.

pub mod address;
pub mod backup;
pub mod calendar;
pub mod checklist;
pub mod config;
pub mod connected_cars;
pub mod danlon;
pub mod db;
pub mod holidays;
pub mod mileage;
pub mod month;
pub mod service;
pub mod state;
pub mod timesheet;
pub mod trip_store;
pub mod trips;
pub mod vacation;

/// Root certificates for HTTPS: the platform's verifier, except on Android,
/// where that verifier needs a JVM hand-off and a Java component the app
/// does not ship; there the bundled Mozilla roots (webpki-roots) are used.
pub(crate) fn root_certs() -> ureq::tls::RootCerts {
    if cfg!(target_os = "android") {
        ureq::tls::RootCerts::WebPki
    } else {
        ureq::tls::RootCerts::PlatformVerifier
    }
}
