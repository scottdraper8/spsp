use std::{
    io,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use super::platform;
use crate::{
    PowerMode,
    store::{ProfileId, ShipId, StateError, StateStore},
};

struct RuntimeStore {
    path: PathBuf,
    state: StateStore,
    writable: bool,
}

static STORE: OnceLock<Mutex<RuntimeStore>> = OnceLock::new();

pub fn remembered(path: PathBuf, profile: ProfileId, ship: ShipId) -> Option<PowerMode> {
    let mutex = STORE.get_or_init(|| Mutex::new(load(path)));
    let store = mutex.lock().ok()?;
    store.state.get(profile, ship)
}

pub fn remember(path: PathBuf, profile: ProfileId, ship: ShipId, mode: PowerMode) {
    let mutex = STORE.get_or_init(|| Mutex::new(load(path)));
    let Ok(mut store) = mutex.lock() else {
        return;
    };
    if !store.writable {
        return;
    }
    let Ok(changed) = store.state.set(profile, ship, mode) else {
        store.writable = false;
        platform::report_diagnostic(
            c"SPSP: sidecar capacity was exceeded; state updates are disabled.\n",
        );
        return;
    };
    if !changed {
        return;
    }
    if let Some(parent) = store.path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        store.writable = false;
        platform::report_diagnostic(
            c"SPSP: could not create the sidecar directory; state updates are disabled.\n",
        );
        return;
    }
    if store.state.save_atomic(&store.path).is_err() {
        store.writable = false;
        platform::report_diagnostic(
            c"SPSP: could not save sidecar state; the previous file was preserved.\n",
        );
    }
}

fn load(path: PathBuf) -> RuntimeStore {
    match StateStore::load(&path) {
        Ok(state) => RuntimeStore {
            path,
            state,
            writable: true,
        },
        Err(StateError::Io(error)) if error.kind() == io::ErrorKind::NotFound => RuntimeStore {
            path,
            state: StateStore::default(),
            writable: true,
        },
        Err(_) => {
            platform::report_diagnostic(c"SPSP: sidecar is invalid or unreadable; it was preserved and state updates are disabled.\n");
            RuntimeStore {
                path,
                state: StateStore::default(),
                writable: false,
            }
        }
    }
}
