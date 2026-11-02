use crate::{
    identity::{
        ResourceIdentity, decode_variable_string, from_resource_fields,
        profile_id as derive_profile_id, unique_ship_id as unique_resource_ship_id,
    },
    store::{ProfileId, ShipId},
};

use super::{platform, supported};

#[derive(Clone, Copy)]
pub struct ActiveShip {
    pub component: usize,
    pub profile: ProfileId,
    pub ship: ShipId,
}

type GetShipComponent = unsafe extern "system" fn(*mut u8, i32) -> *mut u8;

pub fn matches_pattern(base: usize, rva: usize, pattern: &[Option<u8>]) -> bool {
    let Some(address) = base.checked_add(rva) else {
        return false;
    };
    let Some(bytes) = platform::read_bytes(address, pattern.len()) else {
        return false;
    };
    bytes
        .iter()
        .zip(pattern)
        .all(|(actual, expected)| expected.is_none_or(|value| *actual == value))
}

pub fn active_ship_for_component(base: usize, app: usize, component: usize) -> Option<ActiveShip> {
    let data = application_data(base, app)?;
    let index = current_ship_index(data)?;
    let ownership = ship_ownership(data)?;
    let actual = ship_component(base, ownership, index)?;
    if actual != component {
        return None;
    }
    let ship = unique_ship_id(data, index)?;
    let profile = profile_id(app)?;
    Some(ActiveShip {
        component,
        profile,
        ship,
    })
}

pub fn ship_for_spawn(
    base: usize,
    app: usize,
    ownership: usize,
    requested_index: i32,
) -> Option<ActiveShip> {
    let data = application_data(base, app)?;
    let index = match requested_index {
        -1 => current_ship_index(data)?,
        0.. => usize::try_from(requested_index).ok()?,
        _ => return None,
    };
    if index >= supported::PLAYER_SHIPS_COUNT {
        return None;
    }
    let expected_ownership = ship_ownership(data)?;
    if ownership != expected_ownership {
        return None;
    }
    let component = ship_component(base, ownership, index)?;
    if component == 0 {
        return None;
    }
    let ship = unique_ship_id(data, index)?;
    let profile = profile_id(app)?;
    Some(ActiveShip {
        component,
        profile,
        ship,
    })
}

pub fn current_ship(base: usize, app: usize) -> Option<ActiveShip> {
    let data = application_data(base, app)?;
    let index = current_ship_index(data)?;
    let ownership = ship_ownership(data)?;
    let component = ship_component(base, ownership, index)?;
    if component == 0 {
        return None;
    }
    let ship = unique_ship_id(data, index)?;
    let profile = profile_id(app)?;
    Some(ActiveShip {
        component,
        profile,
        ship,
    })
}

fn application_data(base: usize, app: usize) -> Option<usize> {
    if app < 0x10000 {
        return None;
    }
    let data = platform::read_pointer(app.checked_add(supported::APP_DATA_POINTER_OFFSET)?)?;
    let global = platform::read_pointer(base.checked_add(supported::APP_DATA_GLOBAL_RVA)?)?;
    (data == global).then_some(data)
}

fn profile_id(app: usize) -> Option<ProfileId> {
    let slot = platform::read_u32(app.checked_add(supported::APP_SAVE_SLOT_OFFSET)?)?;
    if slot == u32::MAX {
        return None;
    }
    let game_mode = platform::read_u32(app.checked_add(supported::APP_GAME_MODE_OFFSET)?)?;
    derive_profile_id(platform::steam_id()?, slot, game_mode)
}

fn current_ship_index(data: usize) -> Option<usize> {
    let player_state = data
        .checked_add(supported::DATA_GAME_STATE_OFFSET)?
        .checked_add(supported::GAME_STATE_PLAYER_STATE_OFFSET)?;
    let index =
        platform::read_u32(player_state.checked_add(supported::PLAYER_STATE_CURRENT_SHIP_OFFSET)?)?
            as usize;
    (index < supported::PLAYER_SHIPS_COUNT).then_some(index)
}

fn ship_ownership(data: usize) -> Option<usize> {
    data.checked_add(supported::DATA_GAME_STATE_OFFSET)?
        .checked_add(supported::GAME_STATE_SHIP_OWNERSHIP_OFFSET)
}

// The executable's own SpawnNewShip body calls this with its ownership `this` pointer; the
// NMS.py annotation as cGcPlayerState conflicts with that observed call and the 0x68 + index*0x30
// component lookup in the supported executable.
fn ship_component(base: usize, ownership: usize, index: usize) -> Option<usize> {
    if index >= supported::PLAYER_SHIPS_COUNT {
        return None;
    }
    let address = base.checked_add(supported::PLAYER_SHIP_COMPONENT_RVA)?;
    let function: GetShipComponent = unsafe { std::mem::transmute(address) };
    let index = i32::try_from(index).ok()?;
    let component = unsafe { function(ownership as *mut u8, index) } as usize;
    (component >= 0x10000).then_some(component)
}

fn unique_ship_id(data: usize, active_index: usize) -> Option<ShipId> {
    if active_index >= supported::PLAYER_SHIPS_COUNT {
        return None;
    }
    let player_state = data
        .checked_add(supported::DATA_GAME_STATE_OFFSET)?
        .checked_add(supported::GAME_STATE_PLAYER_STATE_OFFSET)?;
    let resources = player_state.checked_add(supported::PLAYER_STATE_SHIP_RESOURCES_OFFSET)?;
    let mut identities = Vec::with_capacity(supported::PLAYER_SHIPS_COUNT);
    for index in 0..supported::PLAYER_SHIPS_COUNT {
        let address = resources.checked_add(index.checked_mul(supported::SHIP_RESOURCE_SIZE)?)?;
        identities.push(resource_identity(address)?);
    }
    unique_resource_ship_id(&identities, active_index)
}

fn resource_identity(address: usize) -> Option<ResourceIdentity> {
    let alt_id = variable_string(address)?;
    let filename = variable_string(address.checked_add(0x10)?)?;
    let seed = platform::read_u64(address.checked_add(0x30)?)?;
    let use_seed = *platform::read::<1>(address.checked_add(0x38)?)?.first()?;
    if use_seed > 1 {
        return None;
    }
    from_resource_fields(&alt_id, &filename, seed, use_seed == 1).ok()
}

fn variable_string(address: usize) -> Option<Vec<u8>> {
    let pointer = usize::try_from(u64::from_le_bytes(platform::read::<8>(address)?)).ok()?;
    let size = platform::read_u32(address.checked_add(8)?)? as usize;
    let capacity = platform::read_u32(address.checked_add(12)?)? as usize;
    if size == 0 {
        return (pointer == 0 || (0x10000..0x0000_8000_0000_0000).contains(&pointer))
            .then(Vec::new);
    }
    if !(0x10000..0x0000_8000_0000_0000).contains(&pointer) || size > 4096 || size > capacity {
        return None;
    }
    let bytes = platform::read_bytes(pointer, size)?;
    decode_variable_string(&bytes, size, capacity)
}
