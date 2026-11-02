use crate::PowerMode;

/// Map the supported NMS build's observed power-mode field to SPSP modes.
///
/// The game indexes its four UI labels directly with this field. In the 7.13 candidate
/// (public Steam build 26385139), values 0 through 3 label Balanced, Guns, Engine, Shield.
pub(crate) fn from_game_value(value: u32) -> Option<PowerMode> {
    match value {
        0 => Some(PowerMode::Balanced),
        1 => Some(PowerMode::Guns),
        2 => Some(PowerMode::Engine),
        3 => Some(PowerMode::Shield),
        _ => None,
    }
}

pub(crate) fn to_game_value(mode: PowerMode) -> u32 {
    match mode {
        PowerMode::Balanced => 0,
        PowerMode::Guns => 1,
        PowerMode::Engine => 2,
        PowerMode::Shield => 3,
    }
}

#[must_use]
pub(crate) fn should_restore(current_value: u32, desired: PowerMode) -> bool {
    from_game_value(current_value).is_some_and(|current| current != desired)
}

#[cfg(test)]
mod tests {
    use super::{from_game_value, should_restore, to_game_value};
    use crate::PowerMode;

    #[test]
    fn candidate_mode_labels_match_the_game_field_values() {
        let expected = [
            PowerMode::Balanced,
            PowerMode::Guns,
            PowerMode::Engine,
            PowerMode::Shield,
        ];
        assert_eq!(PowerMode::ALL, expected);
        for (value, mode) in expected.into_iter().enumerate() {
            let value = u32::try_from(value).unwrap();
            assert_eq!(from_game_value(value), Some(mode));
            assert_eq!(to_game_value(mode), value);
        }
        assert_eq!(from_game_value(4), None);
        assert!(should_restore(0, PowerMode::Guns));
        assert!(!should_restore(1, PowerMode::Guns));
        assert!(!should_restore(4, PowerMode::Guns));
    }
}
