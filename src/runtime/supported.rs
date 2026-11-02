// Static candidate for NMS 7.13, public Steam build 26385139. Hooks remain disabled below.
pub const EXECUTABLE_BYTES: u64 = 89_535_560;
pub const EXECUTABLE_SHA256: [u8; 32] = [
    0xa7, 0x7f, 0xc2, 0x98, 0x77, 0x78, 0x9c, 0x3d, 0xa9, 0x18, 0x1a, 0xfe, 0xb5, 0x94, 0x1b, 0x9b,
    0x85, 0xa2, 0x7d, 0x3d, 0xf2, 0x49, 0x19, 0xa2, 0x44, 0x03, 0xcf, 0x8e, 0x19, 0x4c, 0x61, 0x09,
];

pub const APP_DATA_GLOBAL_RVA: usize = 0x06f6_b208;
pub const APPLICATION_UPDATE_RVA: usize = 0x002d_d3e0;
pub const PLAYER_SHIP_SPAWN_RVA: usize = 0x0056_4cd0;
pub const PLAYER_SHIP_COMPONENT_RVA: usize = 0x0056_8160;
pub const SPACESHIP_UPDATE_CONTROLLED_RVA: usize = 0x0178_df70;
pub const POWER_MODE_STORE_RVA: usize = 0x0178_e849;
pub const POWER_ACTION_RVA: usize = 0x0178_e80b;

// Keep runtime hooks inert until the field mapping, identity stability, and end-to-end restore
// behavior pass controlled validation on a disposable save.
pub const RUNTIME_BINDINGS_VALIDATED: bool = false;

pub const POWER_ACTION_PATTERN: [Option<u8>; 11] = [
    Some(0x41),
    Some(0xb8),
    Some(0x17),
    Some(0x00),
    Some(0x00),
    Some(0x00),
    Some(0xba),
    Some(0x2c),
    Some(0x00),
    Some(0x00),
    Some(0x00),
];

pub const APP_DATA_POINTER_OFFSET: usize = 0x38;
pub const APP_SAVE_SLOT_OFFSET: usize = 0x40;
pub const APP_GAME_MODE_OFFSET: usize = 0x44;
pub const DATA_GAME_STATE_OFFSET: usize = 0x0e70;
pub const GAME_STATE_PLAYER_STATE_OFFSET: usize = 0x0aad0;
pub const GAME_STATE_SHIP_OWNERSHIP_OFFSET: usize = 0x000d_5150;
pub const PLAYER_STATE_SHIP_RESOURCES_OFFSET: usize = 0x17ee0;
pub const PLAYER_STATE_CURRENT_SHIP_OFFSET: usize = 0x182a0;
pub const PLAYER_SHIPS_COUNT: usize = 12;
pub const SHIP_RESOURCE_SIZE: usize = 0x48;
pub const SHIP_POWER_MODE_OFFSET: usize = 0x675c;
pub const SHIP_POWER_TIMER_OFFSET: usize = 0x6760;
pub const POWER_MODE_TIMER_RVA: usize = 0x0723_e854;

pub const APPLICATION_UPDATE_PATTERN: [Option<u8>; 13] = [
    Some(0x40),
    Some(0x53),
    Some(0x48),
    Some(0x83),
    Some(0xec),
    Some(0x20),
    Some(0xe8),
    None,
    None,
    None,
    None,
    Some(0x48),
    Some(0x89),
];

pub const PLAYER_SHIP_SPAWN_PATTERN: [Option<u8>; 15] = [
    Some(0x44),
    Some(0x89),
    Some(0x44),
    Some(0x24),
    None,
    Some(0x55),
    Some(0x53),
    Some(0x56),
    Some(0x57),
    Some(0x41),
    Some(0x54),
    Some(0x41),
    Some(0x55),
    Some(0x41),
    Some(0x57),
];

pub const PLAYER_SHIP_COMPONENT_PATTERN: [Option<u8>; 28] = [
    Some(0x83),
    Some(0xfa),
    None,
    Some(0x75),
    None,
    Some(0x48),
    Some(0x8b),
    Some(0x05),
    None,
    None,
    None,
    None,
    Some(0x8b),
    Some(0x90),
    None,
    None,
    None,
    None,
    Some(0x48),
    Some(0x63),
    Some(0xc2),
    Some(0x48),
    Some(0x8d),
    Some(0x14),
    Some(0x40),
    Some(0x48),
    Some(0x03),
    Some(0xd2),
];

pub const SPACESHIP_UPDATE_CONTROLLED_PATTERN: [Option<u8>; 42] = [
    Some(0xf3),
    Some(0x0f),
    Some(0x11),
    Some(0x4c),
    Some(0x24),
    None,
    Some(0x55),
    Some(0x53),
    Some(0x56),
    Some(0x57),
    Some(0x41),
    Some(0x54),
    Some(0x41),
    Some(0x55),
    Some(0x41),
    Some(0x56),
    Some(0x41),
    Some(0x57),
    Some(0x48),
    Some(0x8d),
    Some(0xac),
    Some(0x24),
    None,
    None,
    None,
    None,
    Some(0xb8),
    None,
    None,
    None,
    None,
    Some(0xe8),
    None,
    None,
    None,
    None,
    Some(0x48),
    Some(0x2b),
    Some(0xe0),
    Some(0x4c),
    Some(0x8b),
    Some(0xa9),
];

const _: () = assert!(!RUNTIME_BINDINGS_VALIDATED);
