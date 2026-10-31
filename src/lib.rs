//! Wire-stable product types shared by SPSP's persistence and runtime layers.

use serde::{Deserialize, Serialize};

pub mod store;

/// A Starship Power Diversion mode recorded in SPSP's external sidecar.
///
/// These string values are SPSP's file format, not assumed numeric game enum discriminants.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerMode {
    Balanced,
    Engine,
    Guns,
    Shield,
}

impl PowerMode {
    /// All supported vanilla modes in the order presented by the game.
    pub const ALL: [Self; 4] = [Self::Balanced, Self::Engine, Self::Guns, Self::Shield];
}

#[cfg(test)]
mod tests {
    use super::PowerMode;

    #[test]
    fn sidecar_names_are_explicit_and_stable() {
        let serialized = PowerMode::ALL.map(|mode| serde_json::to_string(&mode).unwrap());

        assert_eq!(
            serialized,
            ["\"balanced\"", "\"engine\"", "\"guns\"", "\"shield\""]
        );
    }

    #[test]
    fn unknown_modes_are_rejected() {
        assert!(serde_json::from_str::<PowerMode>("\"unknown\"").is_err());
    }
}
