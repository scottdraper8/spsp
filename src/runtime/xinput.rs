use std::ffi::c_void;

use super::platform;

const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;
const ERROR_PROC_NOT_FOUND: u32 = 127;

#[unsafe(no_mangle)]
pub unsafe extern "system" fn XInputGetState(user_index: u32, state: *mut c_void) -> u32 {
    let Some(api) = platform::xinput_api() else {
        return ERROR_DEVICE_NOT_CONNECTED;
    };
    let result = unsafe { (api.get_state)(user_index, state) };
    super::bootstrap();
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn XInputSetState(user_index: u32, vibration: *mut c_void) -> u32 {
    let Some(api) = platform::xinput_api() else {
        return ERROR_PROC_NOT_FOUND;
    };
    let result = unsafe { (api.set_state)(user_index, vibration) };
    super::bootstrap();
    result
}
