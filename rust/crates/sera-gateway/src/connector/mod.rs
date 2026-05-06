//! Connector registry — external channel adapters (Discord, Slack, etc.).
//!
//! The canonical connector surface is defined in `sera-types::connector`.
//! This module re-exports it so gateway code can use a single import path.
//!
//! See SPEC-gateway §8 and `sera-types::connector` for the full design.

pub use sera_types::connector::{
    ChannelConnector, ChannelType, ConnectorError, ConnectorIdentity, ConnectorRegistry,
    ConnectorStatus, OutboundMessage,
};
