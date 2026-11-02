mod game;
mod hook;
mod platform;
mod state;
mod supported;
mod xinput;

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use hook::{HookError, ResidentHook};

struct RuntimeHooks {
    update: Option<ResidentHook>,
    spawn: Option<ResidentHook>,
    mode: Option<ResidentHook>,
}

static HOOKS: OnceLock<Mutex<RuntimeHooks>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static BASE: AtomicUsize = AtomicUsize::new(0);
static APPLICATION: AtomicUsize = AtomicUsize::new(0);
static UPDATE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SPAWN_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static UPDATE_CAPTURED: AtomicBool = AtomicBool::new(false);
static UPDATE_DISABLE_QUEUED: AtomicBool = AtomicBool::new(false);
static STATE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Start exact-build verification and runtime binding from the first `XInput` call.
pub fn bootstrap() {
    if !supported::RUNTIME_BINDINGS_VALIDATED {
        return;
    }
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("spsp-bootstrap".to_owned())
        .spawn(|| {
            let _ = catch_unwind(AssertUnwindSafe(initialize));
        });
    if let Ok(worker) = spawned {
        let _ = worker.join();
    } else {
        STARTED.store(false, Ordering::Release);
    }
}

fn initialize() {
    if !supported::RUNTIME_BINDINGS_VALIDATED {
        return;
    }
    let Ok((_, base)) = platform::verify_executable() else {
        return;
    };
    BASE.store(base, Ordering::Release);
    let Some(path) = platform::state_path() else {
        return;
    };
    let _ = STATE_PATH.set(path);
    if !bindings_match(base) {
        return;
    }
    let Ok(hooks) = prepare_hooks(base) else {
        return;
    };
    install_hooks(hooks);
}

fn bindings_match(base: usize) -> bool {
    game::matches_pattern(
        base,
        supported::APPLICATION_UPDATE_RVA,
        &supported::APPLICATION_UPDATE_PATTERN,
    ) && game::matches_pattern(
        base,
        supported::PLAYER_SHIP_SPAWN_RVA,
        &supported::PLAYER_SHIP_SPAWN_PATTERN,
    ) && game::matches_pattern(
        base,
        supported::PLAYER_SHIP_COMPONENT_RVA,
        &supported::PLAYER_SHIP_COMPONENT_PATTERN,
    ) && game::matches_pattern(
        base,
        supported::SPACESHIP_UPDATE_CONTROLLED_RVA,
        &supported::SPACESHIP_UPDATE_CONTROLLED_PATTERN,
    ) && game::matches_pattern(
        base,
        supported::POWER_ACTION_RVA,
        &supported::POWER_ACTION_PATTERN,
    )
}

fn prepare_hooks(base: usize) -> Result<RuntimeHooks, HookError> {
    let update = unsafe {
        ResidentHook::prepare(
            base + supported::APPLICATION_UPDATE_RVA,
            application_update_detour as *const () as usize,
        )?
    };
    let spawn = unsafe {
        ResidentHook::prepare(
            base + supported::PLAYER_SHIP_SPAWN_RVA,
            player_ship_spawn_detour as *const () as usize,
        )?
    };
    let mode = unsafe {
        ResidentHook::prepare_mode_site(
            base + supported::POWER_MODE_STORE_RVA,
            power_mode_changed as *const () as usize,
        )?
    };
    UPDATE_ORIGINAL.store(update.trampoline(), Ordering::Release);
    SPAWN_ORIGINAL.store(spawn.trampoline(), Ordering::Release);
    Ok(RuntimeHooks {
        update: Some(update),
        spawn: Some(spawn),
        mode: Some(mode),
    })
}

fn install_hooks(hooks: RuntimeHooks) {
    let registry = HOOKS.get_or_init(|| {
        Mutex::new(RuntimeHooks {
            update: None,
            spawn: None,
            mode: None,
        })
    });
    let Ok(mut installed) = registry.lock() else {
        return;
    };
    *installed = hooks;
    if enable_hooks(&mut installed).is_err() {
        rollback_hooks(&mut installed);
        if installed.update.as_ref().is_none_or(|hook| !hook.enabled()) {
            UPDATE_ORIGINAL.store(0, Ordering::Release);
        }
        if installed.spawn.as_ref().is_none_or(|hook| !hook.enabled()) {
            SPAWN_ORIGINAL.store(0, Ordering::Release);
        }
    }
}

fn enable_hooks(hooks: &mut RuntimeHooks) -> Result<(), HookError> {
    let Some(mode) = hooks.mode.as_mut() else {
        return Err(HookError::InvalidTarget);
    };
    unsafe { mode.set_enabled(true)? };
    let Some(spawn) = hooks.spawn.as_mut() else {
        return Err(HookError::InvalidTarget);
    };
    unsafe { spawn.set_enabled(true)? };
    let Some(update) = hooks.update.as_mut() else {
        return Err(HookError::InvalidTarget);
    };
    unsafe { update.set_enabled(true) }
}

fn rollback_hooks(hooks: &mut RuntimeHooks) {
    if let Some(update) = hooks.update.as_mut()
        && update.enabled()
    {
        let _ = unsafe { update.set_enabled(false) };
    }
    if let Some(spawn) = hooks.spawn.as_mut()
        && spawn.enabled()
    {
        let _ = unsafe { spawn.set_enabled(false) };
    }
    if let Some(mode) = hooks.mode.as_mut()
        && mode.enabled()
    {
        let _ = unsafe { mode.set_enabled(false) };
    }
}

unsafe extern "system" fn application_update_detour(application: *mut u8) {
    let first = UPDATE_CAPTURED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if first {
        APPLICATION.store(application as usize, Ordering::Release);
    }
    let original = UPDATE_ORIGINAL.load(Ordering::Acquire);
    if original != 0 {
        let function: unsafe extern "system" fn(*mut u8) = unsafe { std::mem::transmute(original) };
        unsafe { function(application) };
    }
    if first {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            restore_current_ship(application as usize);
        }));
        let _ = catch_unwind(AssertUnwindSafe(disable_update_capture_hook));
    }
}

unsafe extern "system" fn player_ship_spawn_detour(
    ownership: *mut u8,
    matrix: *const u8,
    landing_type: u32,
    ship_index: i32,
    override_position: u8,
) -> u8 {
    type Spawn = unsafe extern "system" fn(*mut u8, *const u8, u32, i32, u8) -> u8;
    let original = SPAWN_ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        return 0;
    }
    let function: Spawn = unsafe { std::mem::transmute(original) };
    let result = unsafe {
        function(
            ownership,
            matrix,
            landing_type,
            ship_index,
            override_position,
        )
    };
    if result != 0 {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            restore_spawned_ship(ownership as usize, ship_index);
        }));
    }
    result
}

unsafe extern "system" fn power_mode_changed(component: *mut u8, mode: u32) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let base = BASE.load(Ordering::Acquire);
        let application = APPLICATION.load(Ordering::Acquire);
        if base == 0 || application == 0 {
            return;
        }
        let Some(mode) = crate::mode_mapping::from_game_value(mode) else {
            return;
        };
        let Some(ship) = game::active_ship_for_component(base, application, component as usize)
        else {
            return;
        };
        if let Some(path) = STATE_PATH.get() {
            state::remember(path.clone(), ship.profile, ship.ship, mode);
        }
    }));
}

fn restore_current_ship(application: usize) {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    if let Some(ship) = game::current_ship(base, application) {
        restore_mode(ship.component, ship.profile, ship.ship);
    }
}

fn restore_spawned_ship(ownership: usize, requested_index: i32) {
    let base = BASE.load(Ordering::Acquire);
    let application = APPLICATION.load(Ordering::Acquire);
    if base == 0 || application == 0 {
        return;
    }
    if let Some(ship) = game::ship_for_spawn(base, application, ownership, requested_index) {
        restore_mode(ship.component, ship.profile, ship.ship);
    }
}

fn restore_mode(component: usize, profile: crate::store::ProfileId, ship: crate::store::ShipId) {
    let Some(path) = STATE_PATH.get() else {
        return;
    };
    let Some(mode) = state::remembered(path.clone(), profile, ship) else {
        return;
    };
    let Some(mode_address) = component.checked_add(supported::SHIP_POWER_MODE_OFFSET) else {
        return;
    };
    let Some(timer_field) = component.checked_add(supported::SHIP_POWER_TIMER_OFFSET) else {
        return;
    };
    let Some(current_mode) = platform::read_u32(mode_address) else {
        return;
    };
    if !crate::mode_mapping::should_restore(current_mode, mode) {
        return;
    }
    let desired_mode = crate::mode_mapping::to_game_value(mode);
    let Some(timer_address) = BASE
        .load(Ordering::Acquire)
        .checked_add(supported::POWER_MODE_TIMER_RVA)
    else {
        return;
    };
    let Some(timer_bits) = platform::read_u32(timer_address) else {
        return;
    };
    if !f32::from_bits(timer_bits).is_finite() {
        return;
    }
    if unsafe { platform::write_u32(mode_address, desired_mode) } {
        let _ = unsafe { platform::write_u32(timer_field, timer_bits) };
    }
}

fn disable_update_capture_hook() {
    let Some(mutex) = HOOKS.get() else {
        return;
    };
    let Ok(mut hooks) = mutex.try_lock() else {
        queue_update_hook_disable();
        return;
    };
    let Some(update) = hooks.update.as_mut() else {
        return;
    };
    if !update.enabled() {
        return;
    }
    if let Err(HookError::Thread(hook::ThreadError::Busy(_))) = unsafe { update.set_enabled(false) }
    {
        queue_update_hook_disable();
    }
}

fn queue_update_hook_disable() {
    if UPDATE_DISABLE_QUEUED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("spsp-hook-disable".to_owned())
        .spawn(|| {
            for _ in 0..100 {
                let Some(mutex) = HOOKS.get() else {
                    break;
                };
                let Ok(mut hooks) = mutex.try_lock() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let Some(update) = hooks.update.as_mut() else {
                    break;
                };
                if !update.enabled() {
                    break;
                }
                match unsafe { update.set_enabled(false) } {
                    Err(HookError::Thread(hook::ThreadError::Busy(_))) => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(()) | Err(_) => break,
                }
            }
            UPDATE_DISABLE_QUEUED.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        UPDATE_DISABLE_QUEUED.store(false, Ordering::Release);
    }
}
