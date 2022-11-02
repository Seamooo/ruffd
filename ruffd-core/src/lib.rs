#[macro_use]
extern crate lazy_static;

mod notifications;
mod requests;
mod ruff_utils;
pub mod server;
mod server_ops;
mod service;

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
pub use service::Service;
