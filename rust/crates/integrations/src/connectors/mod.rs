mod code;
mod productivity;
mod research;
mod system;

use crate::Connector;

/// Instantiate all built-in connectors.
#[must_use]
pub fn all_connectors() -> Vec<Box<dyn Connector>> {
    let mut v: Vec<Box<dyn Connector>> = Vec::new();
    v.extend(code::connectors());
    v.extend(research::connectors());
    v.extend(productivity::connectors());
    v.extend(system::connectors());
    v
}
