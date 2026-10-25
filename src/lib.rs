//! vw-danlon: working time, mileage, vacation and the Danløn app checklist
//! from a work calendar and myVolkswagen trips. Used by the `vw-danlon` CLI
//! and the app; [`service`] holds the steps both run to produce a month's
//! data, the other modules the rules.

pub mod address;
pub mod calendar;
pub mod checklist;
pub mod config;
pub mod connected_cars;
pub mod danlon;
pub mod holidays;
pub mod mileage;
pub mod month;
pub mod service;
pub mod state;
pub mod timesheet;
pub mod trip_store;
pub mod trips;
pub mod vacation;
