use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ZoneId(String);

impl ZoneId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(value)?))
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PortAllocationError {
    #[error("RTSP port allocation overflowed for base {base} and index {index}")]
    Overflow { base: u16, index: usize },
}

pub fn allocate_rtsp_port(base: u16, index: usize) -> Result<u16, PortAllocationError> {
    let index = u16::try_from(index).map_err(|_| PortAllocationError::Overflow { base, index })?;
    base.checked_add(index)
        .ok_or(PortAllocationError::Overflow {
            base,
            index: usize::from(index),
        })
}

pub fn stable_virtual_hwaddr(zone_id: &ZoneId) -> [u8; 6] {
    let digest = Sha256::digest(zone_id.as_str().as_bytes());
    [0x02, digest[0], digest[1], digest[2], digest[3], digest[4]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_deterministic_ports() {
        assert_eq!(allocate_rtsp_port(5000, 0).expect("port"), 5000);
        assert_eq!(allocate_rtsp_port(5000, 5).expect("port"), 5005);
    }

    #[test]
    fn detects_port_overflow() {
        assert!(matches!(
            allocate_rtsp_port(u16::MAX, 1),
            Err(PortAllocationError::Overflow { .. })
        ));
    }

    #[test]
    fn virtual_hwaddr_is_stable_and_locally_administered() {
        let zone = ZoneId::new("RINCON_123");

        let first = stable_virtual_hwaddr(&zone);
        let second = stable_virtual_hwaddr(&zone);

        assert_eq!(first, second);
        assert_eq!(first[0], 0x02);
    }
}
