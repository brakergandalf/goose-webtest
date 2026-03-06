//! goose-webtest: Web application testing agent built on Goose
//!
//! This crate extends Goose with web testing capabilities using the
//! Blueprint pattern (deterministic + agentic hybrid nodes) inspired
//! by Stripe's Minions architecture.

pub mod app_map;
pub mod assertions;
pub mod blueprint;
pub mod config;
pub mod playwright;
pub mod report;
pub mod steps;

pub use blueprint::engine::BlueprintEngine;
pub use config::app_config::AppConfig;
pub use report::TestReport;
