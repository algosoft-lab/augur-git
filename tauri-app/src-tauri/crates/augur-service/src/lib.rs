//! Platform-independent Augur Git application service layer.
//!
//! This crate owns everything both frontends share: the application state, the
//! open-repository sessions, the durable documents, the askpass bridge, the
//! clone pipeline, and the typed command bodies. It never links a UI
//! framework; frontends inject an [`sink::EventSink`] for event delivery and
//! [`store::DocumentStore`] implementations for persistence, and wrap the
//! command bodies with their own transport.

pub mod askpass;
pub mod auto_refresh;
pub mod clone;
pub mod commands;
pub mod credentials;
pub mod error;
pub mod events;
pub mod git_args;
pub mod persistence;
pub mod repo;
pub mod sink;
pub mod state;
pub mod store;
