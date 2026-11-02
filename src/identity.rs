use std::collections::HashSet;

use crate::store::{IdentifierError, ProfileId, ShipId};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ResourceIdentity {
    Empty,
    Identified(ShipId),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
    MissingFilename,
    Identifier(IdentifierError),
}

/// Derive a content key from the candidate persistent resource fields, never a display name.
///
/// The runtime must establish that these fields stay stable across supported ship transitions
/// before enabling this key for persistence.
///
/// # Errors
///
/// Returns an error when a non-empty resource has no filename or the fields exceed identifier
/// limits.
pub(crate) fn from_resource_fields(
    alt_id: &[u8],
    filename: &[u8],
    seed: u64,
    use_seed: bool,
) -> Result<ResourceIdentity, IdentityError> {
    if alt_id.is_empty() && filename.is_empty() && !use_seed {
        return Ok(ResourceIdentity::Empty);
    }
    if filename.is_empty() {
        return Err(IdentityError::MissingFilename);
    }

    let mut tagged_alt_id = Vec::with_capacity(alt_id.len() + 1);
    tagged_alt_id.push(0xa1);
    tagged_alt_id.extend_from_slice(alt_id);
    let mut tagged_filename = Vec::with_capacity(filename.len() + 1);
    tagged_filename.push(0xf1);
    tagged_filename.extend_from_slice(filename);
    let stable_seed = if use_seed { seed } else { 0 };
    let mut seed_component = stable_seed.to_le_bytes().to_vec();
    seed_component.push(u8::from(use_seed));
    let id = ShipId::derive(&[&tagged_alt_id, &tagged_filename, &seed_component])
        .map_err(IdentityError::Identifier)?;
    Ok(ResourceIdentity::Identified(id))
}

#[must_use]
pub(crate) fn profile_id(steam_id: u64, save_slot: u32, game_mode: u32) -> Option<ProfileId> {
    // The supported build's cGcGameMode values are Normal through Seasonal (1..=6).
    if steam_id == 0
        || steam_id == u64::MAX
        || save_slot == u32::MAX
        || !(1..=6).contains(&game_mode)
    {
        return None;
    }
    let steam_component = steam_id.to_le_bytes();
    let slot_component = save_slot.to_le_bytes();
    let mode_component = game_mode.to_le_bytes();
    ProfileId::derive(&[&steam_component, &slot_component, &mode_component]).ok()
}

#[must_use]
pub(crate) fn decode_variable_string(
    bytes: &[u8],
    size: usize,
    capacity: usize,
) -> Option<Vec<u8>> {
    if size == 0 {
        return bytes.is_empty().then(Vec::new);
    }
    if size > 4096 || size > capacity || size != bytes.len() {
        return None;
    }
    let mut value = bytes.to_vec();
    if value.pop() != Some(0) || value.contains(&0) {
        return None;
    }
    Some(value)
}

/// Resolve an active resource only when every candidate key is unambiguous in the owned list.
#[must_use]
pub(crate) fn unique_ship_id(
    identities: &[ResourceIdentity],
    active_index: usize,
) -> Option<ShipId> {
    let ResourceIdentity::Identified(active) = *identities.get(active_index)? else {
        return None;
    };
    let mut seen = HashSet::with_capacity(identities.len());
    for identity in identities {
        if let ResourceIdentity::Identified(id) = identity
            && !seen.insert(*id)
        {
            return None;
        }
    }
    Some(active)
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityError, ResourceIdentity, decode_variable_string, from_resource_fields, profile_id,
        unique_ship_id,
    };

    #[test]
    fn same_resource_fields_produce_the_same_candidate_key() {
        let first = from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 42, true).unwrap();
        let same_resource =
            from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 42, true).unwrap();
        assert!(first == same_resource);
    }

    #[test]
    fn different_resource_fields_produce_distinct_candidate_keys() {
        let first = from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 42, true).unwrap();
        let different_alt =
            from_resource_fields(b"OTHER_ALT", b"MODELS/SHIP.SCENE.MBIN", 42, true).unwrap();
        let different_filename =
            from_resource_fields(b"SHIP_ALT", b"MODELS/OTHER.SCENE.MBIN", 42, true).unwrap();
        assert!(first != different_alt);
        assert!(first != different_filename);
    }

    #[test]
    fn unused_seed_bits_do_not_change_the_candidate_key() {
        let first = from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 1, false).unwrap();
        let second =
            from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", u64::MAX, false).unwrap();
        assert!(first == second);
    }

    #[test]
    fn enabled_seed_participates_in_the_candidate_key() {
        let first = from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 1, true).unwrap();
        let second = from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 2, true).unwrap();
        assert!(first != second);
    }

    #[test]
    fn incomplete_resources_fail_closed_and_empty_slots_are_explicit() {
        assert!(from_resource_fields(b"", b"", 0, false).unwrap() == ResourceIdentity::Empty);
        assert!(
            from_resource_fields(b"SHIP_ALT", b"", 0, false) == Err(IdentityError::MissingFilename)
        );
    }

    #[test]
    fn profile_identity_separates_accounts_slots_and_modes() {
        let first = profile_id(123_456, 2, 1).unwrap();
        assert!(first == profile_id(123_456, 2, 1).unwrap());
        assert!(first != profile_id(123_456, 3, 1).unwrap());
        assert!(first != profile_id(123_456, 2, 2).unwrap());
        assert!(first != profile_id(654_321, 2, 1).unwrap());
        assert!(profile_id(0, 2, 2).is_none());
        assert!(profile_id(123_456, u32::MAX, 2).is_none());
        assert!(profile_id(123_456, 2, 0).is_none());
        assert!(profile_id(123_456, 2, 7).is_none());
    }

    #[test]
    fn variable_strings_require_bounded_capacity_and_one_terminal_nul() {
        assert_eq!(decode_variable_string(b"SHIP\0", 5, 8).unwrap(), b"SHIP");
        assert!(decode_variable_string(b"SHIP", 5, 8).is_none());
        assert!(decode_variable_string(b"SH\0IP\0", 6, 8).is_none());
        assert!(decode_variable_string(b"SHIP\0", 5, 4).is_none());
        assert!(decode_variable_string(b"SHIP\0", 4, 8).is_none());
        assert!(decode_variable_string(&[b'x'; 4097], 4097, 4097).is_none());
        assert!(decode_variable_string(b"", 0, 0).unwrap().is_empty());
    }

    #[test]
    fn duplicates_and_empty_active_slots_do_not_resolve() {
        let identity =
            from_resource_fields(b"SHIP_ALT", b"MODELS/SHIP.SCENE.MBIN", 42, true).unwrap();
        let other =
            from_resource_fields(b"OTHER_ALT", b"MODELS/OTHER.SCENE.MBIN", 9, true).unwrap();
        assert!(unique_ship_id(&[identity, identity], 0).is_none());
        assert!(unique_ship_id(&[ResourceIdentity::Empty, identity], 0).is_none());
        assert!(unique_ship_id(&[identity, other], 1).is_some());
        assert!(unique_ship_id(&[identity], 1).is_none());
    }
}
