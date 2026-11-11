//! Direct-connection model registry: providers, accounts and model routes.
//!
//! Replaces the CLI-centric `providers`/`provider_registry` pair. Nothing here
//! spawns a process — a provider is an HTTP endpoint plus the credentials found
//! for it.

pub mod commands;
pub mod presets;
pub mod registry;
pub mod store;
pub mod types;
