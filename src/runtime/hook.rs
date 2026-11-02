use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Decoder, DecoderOptions, FlowControl, InstructionBlock,
};
use std::{
    ffi::c_void,
    fmt::{self, Display, Formatter},
    mem, ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, STILL_ACTIVE},
    System::{
        Diagnostics::Debug::{
            CONTEXT, CONTEXT_CONTROL_AMD64, FlushInstructionCache, GetThreadContext,
        },
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
        },
        Memory::{
            MEM_COMMIT, MEM_FREE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION,
            PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
            VirtualProtect, VirtualQuery,
        },
        Threading::{
            GetCurrentProcess, GetCurrentThreadId, GetExitCodeThread, GetThreadId, ResumeThread,
            SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
            THREAD_SUSPEND_RESUME,
        },
    },
};

const JUMP_SIZE: usize = 14;
const PAGE_SIZE: usize = 4096;
const THREAD_LIMIT: usize = 4096;
const ALLOCATION_GRANULARITY: usize = 0x10000;
const NEAR_ALLOCATION_RANGE: usize = 0x0700_0000;
const MODE_PREFIX: [u8; 22] = [
    0x89, 0x9e, 0x5c, 0x67, 0x00, 0x00, 0xf3, 0x0f, 0x10, 0x05, 0xfd, 0xff, 0xaa, 0x05, 0xf3, 0x0f,
    0x11, 0x86, 0x60, 0x67, 0x00, 0x00,
];

#[derive(Debug)]
pub enum HookError {
    InvalidTarget,
    InvalidInstruction,
    UnsupportedControlFlow,
    Relocation,
    PrefixChanged,
    Allocation(u32),
    ModulePin(u32),
    Protection(u32),
    Flush(u32),
    Thread(ThreadError),
    CodeCapacity,
}

/// A process-lifetime x64 code detour. The SPSP module is pinned before the patch is enabled;
/// the trampoline remains allocated so an in-flight detour can finish during shutdown.
pub struct ResidentHook {
    target: usize,
    original: Vec<u8>,
    patched: Vec<u8>,
    trampoline: usize,
    enabled: bool,
}

// The code page and patch state are accessed only while the owning hook is serialized.
unsafe impl Send for ResidentHook {}

impl ResidentHook {
    /// Prepare a standard function-entry detour with an iced-x86 relocated trampoline.
    ///
    /// # Safety
    /// `target` must be a verified straight-line function entry in the loaded game image, and
    /// `destination` must be a callable function with the exact target ABI.
    pub unsafe fn prepare(target: usize, destination: usize) -> Result<Self, HookError> {
        pin_module(destination)?;
        let (original, prefix_len, instructions) = decode_prefix(target, JUMP_SIZE)?;
        let allocation = allocate_page_near(target)?;
        let trampoline = allocation as usize;
        let relocated = match relocate(
            &instructions,
            trampoline as u64,
            (target + prefix_len) as u64,
        ) {
            Ok(code) => code,
            Err(error) => {
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                return Err(error);
            }
        };
        if relocated.len() > PAGE_SIZE {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(HookError::CodeCapacity);
        }
        unsafe {
            ptr::copy_nonoverlapping(relocated.as_ptr(), allocation.cast::<u8>(), relocated.len());
        }
        if let Err(error) = protect_code(allocation, relocated.len()) {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(error);
        }
        let patched = match make_patch(prefix_len, destination as u64) {
            Ok(patch) => patch,
            Err(error) => {
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                return Err(error);
            }
        };
        Ok(Self {
            target,
            original,
            patched,
            trampoline,
            enabled: false,
        })
    }

    /// Prepare the precise post-input state-change site in `cGcSpaceshipComponent::UpdateControlled`.
    /// The bridge reports the new mode before replaying the original store and timer update.
    ///
    /// # Safety
    /// `target` and `callback` must be the exact-build site and SPSP callback respectively.
    pub unsafe fn prepare_mode_site(target: usize, callback: usize) -> Result<Self, HookError> {
        pin_module(callback)?;
        let observed = read_code(target, MODE_PREFIX.len())?;
        if observed != MODE_PREFIX {
            return Err(HookError::PrefixChanged);
        }
        let instructions = decode_exact(target, &MODE_PREFIX)?;
        let allocation = allocate_page_near(target)?;
        let trampoline = allocation as usize;
        let relocated = match relocate(
            &instructions,
            trampoline as u64,
            (target + MODE_PREFIX.len()) as u64,
        ) {
            Ok(code) => code,
            Err(error) => {
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                return Err(error);
            }
        };
        let bridge_offset = 2048usize;
        let Some(bridge_address) = trampoline.checked_add(bridge_offset) else {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(HookError::CodeCapacity);
        };
        let bridge = mode_bridge(callback as u64, trampoline as u64);
        if relocated.len() > bridge_offset || bridge_offset + bridge.len() > PAGE_SIZE {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(HookError::CodeCapacity);
        }
        unsafe {
            ptr::copy_nonoverlapping(relocated.as_ptr(), allocation.cast::<u8>(), relocated.len());
            ptr::copy_nonoverlapping(
                bridge.as_ptr(),
                allocation.cast::<u8>().add(bridge_offset),
                bridge.len(),
            );
        }
        if let Err(error) = protect_code(allocation, bridge_offset + bridge.len()) {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(error);
        }
        let patched = match make_patch(MODE_PREFIX.len(), bridge_address as u64) {
            Ok(patch) => patch,
            Err(error) => {
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                return Err(error);
            }
        };
        Ok(Self {
            target,
            original: MODE_PREFIX.to_vec(),
            patched,
            trampoline,
            enabled: false,
        })
    }

    pub fn trampoline(&self) -> usize {
        self.trampoline
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// # Safety
    /// The target belongs to the exact supported executable. All callers and code pages remain
    /// live, and this hook is the sole owner of the patched target prefix.
    pub unsafe fn set_enabled(&mut self, enabled: bool) -> Result<(), HookError> {
        if self.enabled == enabled {
            return Ok(());
        }
        let (expected, replacement) = if enabled {
            (&self.original, &self.patched)
        } else {
            (&self.patched, &self.original)
        };
        let frozen =
            FrozenThreads::outside(self.target, expected.len()).map_err(HookError::Thread)?;
        if read_code(self.target, expected.len())? != *expected {
            return Err(HookError::PrefixChanged);
        }
        let mut old_protection = 0;
        if unsafe {
            VirtualProtect(
                self.target as *mut c_void,
                replacement.len(),
                PAGE_EXECUTE_READWRITE,
                &raw mut old_protection,
            )
        } == 0
        {
            return Err(HookError::Protection(unsafe { GetLastError() }));
        }
        unsafe {
            ptr::copy_nonoverlapping(
                replacement.as_ptr(),
                self.target as *mut u8,
                replacement.len(),
            );
        }
        let flushed = unsafe {
            FlushInstructionCache(
                GetCurrentProcess(),
                self.target as *const c_void,
                replacement.len(),
            )
        } != 0;
        if flushed {
            self.enabled = enabled;
        } else {
            unsafe {
                ptr::copy_nonoverlapping(expected.as_ptr(), self.target as *mut u8, expected.len());
                FlushInstructionCache(
                    GetCurrentProcess(),
                    self.target as *const c_void,
                    expected.len(),
                );
            }
        }
        let mut discarded = 0;
        let protected = unsafe {
            VirtualProtect(
                self.target as *mut c_void,
                replacement.len(),
                old_protection,
                &raw mut discarded,
            )
        } != 0;
        frozen.resume().map_err(HookError::Thread)?;
        if !flushed {
            return Err(HookError::Flush(unsafe { GetLastError() }));
        }
        if !protected {
            return Err(HookError::Protection(unsafe { GetLastError() }));
        }
        Ok(())
    }
}

fn pin_module(address: usize) -> Result<(), HookError> {
    let mut module = ptr::null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            address as *const u16,
            &raw mut module,
        )
    } == 0
    {
        return Err(HookError::ModulePin(unsafe { GetLastError() }));
    }
    Ok(())
}

fn allocate_page_near(target: usize) -> Result<*mut c_void, HookError> {
    let lower_bound = target.saturating_sub(NEAR_ALLOCATION_RANGE);
    let upper_bound = target.saturating_add(NEAR_ALLOCATION_RANGE);
    let mut cursor = lower_bound & !(ALLOCATION_GRANULARITY - 1);
    while cursor < upper_bound {
        let mut region: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        let queried = unsafe {
            VirtualQuery(
                cursor as *const c_void,
                &raw mut region,
                mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if queried == 0 {
            cursor = cursor.saturating_add(ALLOCATION_GRANULARITY);
            continue;
        }
        let region_start = region.BaseAddress as usize;
        let region_end = region_start.saturating_add(region.RegionSize);
        if region.State == MEM_FREE {
            let start = region_start.max(lower_bound);
            let candidate =
                start.saturating_add(ALLOCATION_GRANULARITY - 1) & !(ALLOCATION_GRANULARITY - 1);
            let limit = region_end.min(upper_bound);
            let mut candidate = candidate;
            while candidate.saturating_add(PAGE_SIZE) <= limit {
                let allocation = unsafe {
                    VirtualAlloc(
                        candidate as *mut c_void,
                        PAGE_SIZE,
                        MEM_COMMIT | MEM_RESERVE,
                        PAGE_READWRITE,
                    )
                };
                if !allocation.is_null() {
                    return Ok(allocation);
                }
                candidate = candidate.saturating_add(ALLOCATION_GRANULARITY);
            }
        }
        cursor = region_end.max(cursor.saturating_add(ALLOCATION_GRANULARITY));
    }
    Err(HookError::Allocation(unsafe { GetLastError() }))
}

fn protect_code(allocation: *mut c_void, length: usize) -> Result<(), HookError> {
    let mut old = 0;
    if unsafe { VirtualProtect(allocation, PAGE_SIZE, PAGE_EXECUTE_READ, &raw mut old) } == 0 {
        return Err(HookError::Protection(unsafe { GetLastError() }));
    }
    if unsafe { FlushInstructionCache(GetCurrentProcess(), allocation, length) } == 0 {
        return Err(HookError::Flush(unsafe { GetLastError() }));
    }
    Ok(())
}

fn read_code(address: usize, length: usize) -> Result<Vec<u8>, HookError> {
    if address < 0x10000 || length == 0 || length > 64 {
        return Err(HookError::InvalidTarget);
    }
    let mut bytes = vec![0u8; length];
    let mut copied = 0;
    let ok = unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            length,
            &raw mut copied,
        )
    };
    if ok == 0 || copied != length {
        return Err(HookError::InvalidTarget);
    }
    Ok(bytes)
}

fn decode_prefix(
    target: usize,
    minimum: usize,
) -> Result<(Vec<u8>, usize, Vec<iced_x86::Instruction>), HookError> {
    let code = read_code(target, 64)?;
    let mut decoder = Decoder::with_ip(64, &code, target as u64, DecoderOptions::NONE);
    let mut instructions = Vec::new();
    while decoder.position() < minimum {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(HookError::InvalidInstruction);
        }
        if !matches!(
            instruction.flow_control(),
            FlowControl::Next | FlowControl::Call
        ) {
            return Err(HookError::UnsupportedControlFlow);
        }
        instructions.push(instruction);
    }
    let prefix_len = decoder.position();
    Ok((code[..prefix_len].to_vec(), prefix_len, instructions))
}

fn decode_exact(target: usize, code: &[u8]) -> Result<Vec<iced_x86::Instruction>, HookError> {
    let mut decoder = Decoder::with_ip(64, code, target as u64, DecoderOptions::NONE);
    let mut instructions = Vec::new();
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.is_invalid() {
            return Err(HookError::InvalidInstruction);
        }
        if !matches!(
            instruction.flow_control(),
            FlowControl::Next | FlowControl::Call
        ) {
            return Err(HookError::UnsupportedControlFlow);
        }
        instructions.push(instruction);
    }
    if decoder.position() != code.len() {
        return Err(HookError::InvalidInstruction);
    }
    Ok(instructions)
}

fn relocate(
    instructions: &[iced_x86::Instruction],
    destination: u64,
    continuation: u64,
) -> Result<Vec<u8>, HookError> {
    let encoded = BlockEncoder::encode(
        64,
        InstructionBlock::new(instructions, destination),
        BlockEncoderOptions::NONE,
    )
    .map_err(|_| HookError::Relocation)?;
    let mut code = encoded.code_buffer;
    code.extend_from_slice(&absolute_jump(continuation));
    Ok(code)
}

fn make_patch(prefix_len: usize, destination: u64) -> Result<Vec<u8>, HookError> {
    if !(JUMP_SIZE..=64).contains(&prefix_len) {
        return Err(HookError::InvalidTarget);
    }
    let mut patch = vec![0x90; prefix_len];
    patch[..JUMP_SIZE].copy_from_slice(&absolute_jump(destination));
    Ok(patch)
}

fn absolute_jump(destination: u64) -> [u8; JUMP_SIZE] {
    let mut code = [0u8; JUMP_SIZE];
    code[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
    code[6..].copy_from_slice(&destination.to_le_bytes());
    code
}

#[cfg(test)]
mod tests {
    use iced_x86::{Decoder, DecoderOptions, FlowControl, Mnemonic};

    use super::{JUMP_SIZE, MODE_PREFIX, absolute_jump, decode_exact, make_patch, mode_bridge};
    use crate::runtime::supported;

    #[test]
    fn mode_site_is_exactly_three_supported_instructions() {
        let decoded = decode_exact(0x1417_8e849, &MODE_PREFIX).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].mnemonic(), Mnemonic::Mov);
        assert_eq!(decoded[1].mnemonic(), Mnemonic::Movss);
        assert_eq!(decoded[2].mnemonic(), Mnemonic::Movss);
        assert!(
            decoded
                .iter()
                .all(|instruction| { matches!(instruction.flow_control(), FlowControl::Next) })
        );
    }

    #[test]
    fn mode_timer_rva_matches_the_rip_relative_load_in_the_supported_site() {
        let image_base = 0x1_4000_0000_usize;
        let instruction_end = image_base + supported::POWER_MODE_STORE_RVA + 14;
        let displacement = i32::from_le_bytes(MODE_PREFIX[10..14].try_into().unwrap());
        let timer_address = instruction_end
            .checked_add_signed(isize::try_from(displacement).unwrap())
            .unwrap();
        assert_eq!(timer_address - image_base, supported::POWER_MODE_TIMER_RVA);
    }

    #[test]
    fn absolute_jump_embeds_its_destination_after_the_indirect_jump() {
        let destination = 0x1234_5678_9abc_def0;
        let jump = absolute_jump(destination);
        assert_eq!(&jump[..6], &[0xff, 0x25, 0, 0, 0, 0]);
        assert_eq!(
            u64::from_le_bytes(jump[6..].try_into().unwrap()),
            destination
        );
        assert_eq!(jump.len(), JUMP_SIZE);
    }

    #[test]
    fn function_patch_preserves_prefix_length_and_nops_the_remainder() {
        let patch = make_patch(22, 0x1234).unwrap();
        assert_eq!(patch.len(), 22);
        assert_eq!(&patch[..JUMP_SIZE], &absolute_jump(0x1234));
        assert!(patch[JUMP_SIZE..].iter().all(|byte| *byte == 0x90));
        assert!(make_patch(JUMP_SIZE - 1, 0x1234).is_err());
        assert!(make_patch(65, 0x1234).is_err());
    }

    #[test]
    fn mode_bridge_is_decodable_and_targets_callback_and_trampoline() {
        let callback = 0x1111_2222_3333_4444;
        let trampoline = 0x5555_6666_7777_8888;
        let bridge = mode_bridge(callback, trampoline);
        let jump_start = bridge.len() - JUMP_SIZE;
        let callback_bytes = callback.to_le_bytes();
        assert!(
            bridge
                .windows(10)
                .any(|bytes| { bytes[..2] == [0x48, 0xb8] && bytes[2..] == callback_bytes })
        );
        assert!(
            bridge
                .windows(4)
                .any(|bytes| bytes == [0x48, 0x89, 0xf1, 0x89])
        );
        assert_eq!(
            u64::from_le_bytes(bridge[bridge.len() - 8..].try_into().unwrap()),
            trampoline
        );

        let mut decoder = Decoder::with_ip(64, &bridge[..jump_start], 0x1000, DecoderOptions::NONE);
        while decoder.can_decode() {
            assert!(!decoder.decode().is_invalid());
        }
        let mut jump_decoder = Decoder::with_ip(
            64,
            &bridge[jump_start..jump_start + 6],
            0x2000,
            DecoderOptions::NONE,
        );
        let jump = jump_decoder.decode();
        assert_eq!(jump.mnemonic(), Mnemonic::Jmp);
        assert_eq!(jump.flow_control(), FlowControl::IndirectBranch);
    }
}

fn mode_bridge(callback: u64, trampoline: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(256);
    code.extend_from_slice(&[
        0x9c, // pushfq
        0x50, // push rax
        0x51, // push rcx
        0x52, // push rdx
        0x41, 0x50, // push r8
        0x41, 0x51, // push r9
        0x41, 0x52, // push r10
        0x41, 0x53, // push r11
        0x48, 0x81, 0xec, 0x80, 0, 0, 0, // reserve shadow space and XMM saves
    ]);
    for register in 0..6 {
        code.extend_from_slice(&[
            0x0f,
            0x11,
            0x44 + (register << 3),
            0x24,
            0x20 + register * 0x10,
        ]);
    }
    code.extend_from_slice(&[
        0x48, 0x89, 0xf1, // mov rcx, rsi
        0x89, 0xda, // mov edx, ebx
        0x48, 0xb8, // mov rax, callback
    ]);
    code.extend_from_slice(&callback.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call rax
    for register in 0..6 {
        code.extend_from_slice(&[
            0x0f,
            0x10,
            0x44 + (register << 3),
            0x24,
            0x20 + register * 0x10,
        ]);
    }
    code.extend_from_slice(&[
        0x48, 0x81, 0xc4, 0x80, 0, 0, 0, // release shadow space and XMM saves
        0x41, 0x5b, // pop r11
        0x41, 0x5a, // pop r10
        0x41, 0x59, // pop r9
        0x41, 0x58, // pop r8
        0x5a, // pop rdx
        0x59, // pop rcx
        0x58, // pop rax
        0x9d, // popfq
    ]);
    code.extend_from_slice(&absolute_jump(trampoline));
    code
}

#[derive(Debug)]
pub enum ThreadError {
    Enumerate(u32),
    Suspend(u32),
    Context(u32),
    Resume(u32),
    Busy(u32),
    Limit,
    Unstable,
}

impl Display for HookError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget => formatter.write_str("invalid hook target"),
            Self::InvalidInstruction => formatter.write_str("invalid x64 instruction"),
            Self::UnsupportedControlFlow => formatter.write_str("unsupported control flow"),
            Self::Relocation => formatter.write_str("could not relocate hook prefix"),
            Self::PrefixChanged => formatter.write_str("hook prefix changed"),
            Self::Allocation(code) => write!(formatter, "code allocation failed ({code})"),
            Self::ModulePin(code) => write!(formatter, "could not pin SPSP module ({code})"),
            Self::Protection(code) => write!(formatter, "memory protection change failed ({code})"),
            Self::Flush(code) => write!(formatter, "instruction cache flush failed ({code})"),
            Self::Thread(error) => write!(formatter, "thread coordination failed: {error}"),
            Self::CodeCapacity => formatter.write_str("hook code exceeds allocated page"),
        }
    }
}

impl Display for ThreadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumerate(code) => write!(formatter, "thread enumeration failed ({code})"),
            Self::Suspend(code) => write!(formatter, "thread suspension failed ({code})"),
            Self::Context(code) => write!(formatter, "thread context query failed ({code})"),
            Self::Resume(code) => write!(formatter, "thread resume failed ({code})"),
            Self::Busy(id) => write!(formatter, "thread {id} is executing patched code"),
            Self::Limit => formatter.write_str("thread limit exceeded"),
            Self::Unstable => formatter.write_str("thread set did not stabilize"),
        }
    }
}

struct Suspended {
    id: u32,
    handle: HANDLE,
}

struct FrozenThreads {
    threads: Vec<Suspended>,
}

type NextThread = unsafe extern "system" fn(HANDLE, HANDLE, u32, u32, u32, *mut HANDLE) -> i32;

fn thread_exited(handle: HANDLE) -> bool {
    let mut code = STILL_ACTIVE as u32;
    unsafe { GetExitCodeThread(handle, &raw mut code) != 0 && code != STILL_ACTIVE as u32 }
}

impl FrozenThreads {
    fn outside(target: usize, length: usize) -> Result<Self, ThreadError> {
        let ntdll = unsafe { GetModuleHandleW(windows_sys::w!("ntdll.dll")) };
        if ntdll.is_null() {
            return Err(ThreadError::Enumerate(unsafe { GetLastError() }));
        }
        let next = unsafe { GetProcAddress(ntdll, c"NtGetNextThread".as_ptr().cast()) }
            .ok_or(ThreadError::Enumerate(unsafe { GetLastError() }))?;
        let next =
            unsafe { mem::transmute::<unsafe extern "system" fn() -> isize, NextThread>(next) };
        let mut frozen = Self {
            threads: Vec::with_capacity(THREAD_LIMIT),
        };
        let current = unsafe { GetCurrentThreadId() };
        for _ in 0..4 {
            let before = frozen.threads.len();
            frozen.freeze_pass(next, current, target, length)?;
            if frozen.threads.len() == before {
                return Ok(frozen);
            }
        }
        Err(ThreadError::Unstable)
    }

    fn freeze_pass(
        &mut self,
        next: NextThread,
        current: u32,
        target: usize,
        length: usize,
    ) -> Result<(), ThreadError> {
        let mut cursor = ptr::null_mut();
        let mut retained = false;
        loop {
            let mut handle = ptr::null_mut();
            let status = unsafe {
                next(
                    GetCurrentProcess(),
                    cursor,
                    THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME,
                    0,
                    0,
                    &raw mut handle,
                )
            };
            if !cursor.is_null() && !retained {
                unsafe { CloseHandle(cursor) };
            }
            if status.cast_unsigned() == 0x8000_001a {
                return Ok(());
            }
            if status != 0 {
                return Err(ThreadError::Enumerate(status.cast_unsigned()));
            }
            cursor = handle;
            retained = false;
            let id = unsafe { GetThreadId(handle) };
            if id == current || self.threads.iter().any(|thread| thread.id == id) {
                continue;
            }
            if self.threads.len() == THREAD_LIMIT {
                unsafe { CloseHandle(handle) };
                return Err(ThreadError::Limit);
            }
            if unsafe { SuspendThread(handle) } == u32::MAX {
                let error = unsafe { GetLastError() };
                if thread_exited(handle) {
                    continue;
                }
                unsafe { CloseHandle(handle) };
                return Err(ThreadError::Suspend(error));
            }
            self.threads.push(Suspended { id, handle });
            retained = true;
            let mut context: CONTEXT = unsafe { mem::zeroed() };
            context.ContextFlags = CONTEXT_CONTROL_AMD64;
            if unsafe { GetThreadContext(handle, &raw mut context) } == 0 && !thread_exited(handle)
            {
                return Err(ThreadError::Context(unsafe { GetLastError() }));
            }
            if usize::try_from(context.Rip).is_ok_and(|ip| (target..target + length).contains(&ip))
            {
                return Err(ThreadError::Busy(id));
            }
        }
    }

    fn resume(mut self) -> Result<(), ThreadError> {
        self.resume_all()
    }

    fn resume_all(&mut self) -> Result<(), ThreadError> {
        let mut failure = None;
        for thread in self.threads.iter_mut().rev() {
            if thread.handle.is_null() {
                continue;
            }
            if unsafe { ResumeThread(thread.handle) } == u32::MAX && !thread_exited(thread.handle) {
                failure = Some(ThreadError::Resume(unsafe { GetLastError() }));
                continue;
            }
            unsafe { CloseHandle(thread.handle) };
            thread.handle = ptr::null_mut();
        }
        failure.map_or(Ok(()), Err)
    }
}

impl Drop for FrozenThreads {
    fn drop(&mut self) {
        let _ = self.resume_all();
    }
}
