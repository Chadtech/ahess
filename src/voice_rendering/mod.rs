//! Instrument synthesis and instrument-specific rendering support.

pub(crate) mod clarinet;
pub(crate) mod gamelan_metallophone;
mod historical_convolution;
pub(crate) mod noitech_bell_a;
pub(crate) mod noitech_bell_b;
pub(crate) mod recovered_voice;
#[cfg(target_os = "macos")]
pub(crate) mod surge_xt;
pub(crate) mod vsco;
