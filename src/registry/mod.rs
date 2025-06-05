// This file defines the registry module for handling Docker registry communication.

pub mod auth;
pub mod client;

pub use auth::{Auth, AuthChallenge};
pub use client::RegistryClient;