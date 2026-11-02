use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, OsString, c_void},
    fs::File,
    io::{self, Read},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    ptr,
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{OutputDebugStringA, ReadProcessMemory, WriteProcessMemory},
    LibraryLoader::{GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW},
    SystemInformation::GetSystemDirectoryW,
    Threading::GetCurrentProcess,
};

use super::supported::{EXECUTABLE_BYTES, EXECUTABLE_SHA256};

pub fn verify_executable() -> Result<(PathBuf, usize), &'static str> {
    let path = executable_path().map_err(|_| "cannot locate the running executable")?;
    if path
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case("NMS.exe"))
    {
        return Err("the current process is not NMS.exe");
    }
    let mut file = File::open(&path).map_err(|_| "cannot open the running executable")?;
    if file
        .metadata()
        .map_err(|_| "cannot inspect the running executable")?
        .len()
        != EXECUTABLE_BYTES
    {
        return Err("unsupported NMS executable size");
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "cannot hash the running executable")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual = hash.finalize();
    if actual.as_slice() != EXECUTABLE_SHA256 {
        return Err("unsupported NMS executable hash");
    }
    let base = unsafe { GetModuleHandleW(ptr::null()) } as usize;
    if base == 0 {
        return Err("NMS.exe is not mapped");
    }
    Ok((path, base))
}

pub fn report_diagnostic(message: &'static CStr) {
    unsafe { OutputDebugStringA(message.as_ptr().cast()) };
}

pub fn state_path() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    root.is_absolute()
        .then(|| root.join("SPSP").join("NoMansSky").join("state.json"))
}

pub fn executable_path() -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    let capacity = u32::try_from(buffer.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path buffer too large"))?;
    let length =
        unsafe { GetModuleFileNameW(ptr::null_mut(), buffer.as_mut_ptr(), capacity) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

pub fn read<const N: usize>(address: usize) -> Option<[u8; N]> {
    let bytes = read_bytes(address, N)?;
    bytes.try_into().ok()
}

pub fn read_bytes(address: usize, length: usize) -> Option<Vec<u8>> {
    if address < 0x10000
        || length == 0
        || address
            .checked_add(length)
            .is_none_or(|end| end > 0x0000_8000_0000_0000)
    {
        return None;
    }
    let mut bytes = vec![0u8; length];
    let mut copied = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            length,
            &raw mut copied,
        )
    };
    (ok != 0 && copied == length).then_some(bytes)
}

pub fn read_pointer(address: usize) -> Option<usize> {
    let raw = usize::try_from(u64::from_le_bytes(read::<8>(address)?)).ok()?;
    (0x10000..0x0000_8000_0000_0000)
        .contains(&raw)
        .then_some(raw)
}

pub fn read_u32(address: usize) -> Option<u32> {
    Some(u32::from_le_bytes(read::<4>(address)?))
}

pub fn read_u64(address: usize) -> Option<u64> {
    Some(u64::from_le_bytes(read::<8>(address)?))
}

/// # Safety
/// `address` must be an exact-build writable game field of at least four bytes.
pub unsafe fn write_u32(address: usize, value: u32) -> bool {
    let bytes = value.to_le_bytes();
    let mut copied = 0;
    let ok = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *mut c_void,
            bytes.as_ptr().cast(),
            bytes.len(),
            &raw mut copied,
        )
    };
    ok != 0 && copied == bytes.len() && read::<4>(address) == Some(bytes)
}

pub fn steam_id() -> Option<u64> {
    type SteamUser = unsafe extern "system" fn() -> *mut c_void;
    type GetSteamId = unsafe extern "system" fn(*mut c_void) -> u64;

    let module = unsafe { GetModuleHandleW(windows_sys::w!("steam_api64.dll")) };
    if module.is_null() {
        return None;
    }
    let user = unsafe { GetProcAddress(module, c"SteamAPI_SteamUser_v021".as_ptr().cast()) }?;
    let get_id =
        unsafe { GetProcAddress(module, c"SteamAPI_ISteamUser_GetSteamID".as_ptr().cast()) }?;
    let user: SteamUser = unsafe { std::mem::transmute(user) };
    let get_id: GetSteamId = unsafe { std::mem::transmute(get_id) };
    let interface = unsafe { user() };
    if interface.is_null() {
        return None;
    }
    let id = unsafe { get_id(interface) };
    (id != 0 && id != u64::MAX).then_some(id)
}

pub struct XInputApi {
    _module: usize,
    pub get_state: unsafe extern "system" fn(u32, *mut c_void) -> u32,
    pub set_state: unsafe extern "system" fn(u32, *mut c_void) -> u32,
}

pub fn xinput_api() -> Option<&'static XInputApi> {
    static API: std::sync::OnceLock<Option<XInputApi>> = std::sync::OnceLock::new();
    API.get_or_init(load_xinput).as_ref()
}

fn load_xinput() -> Option<XInputApi> {
    let mut path = vec![0u16; 32_768];
    let capacity = u32::try_from(path.len()).ok()?;
    let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), capacity) } as usize;
    if length == 0 || length >= path.len() {
        return None;
    }
    path.truncate(length);
    path.extend("\\xinput9_1_0.dll".encode_utf16());
    path.push(0);
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        return None;
    }
    let get_state = unsafe { GetProcAddress(module, c"XInputGetState".as_ptr().cast()) }?;
    let set_state = unsafe { GetProcAddress(module, c"XInputSetState".as_ptr().cast()) }?;
    let get_state: unsafe extern "system" fn(u32, *mut c_void) -> u32 = unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(u32, *mut c_void) -> u32,
        >(get_state)
    };
    let set_state: unsafe extern "system" fn(u32, *mut c_void) -> u32 = unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(u32, *mut c_void) -> u32,
        >(set_state)
    };
    Some(XInputApi {
        _module: module as usize,
        get_state,
        set_state,
    })
}
