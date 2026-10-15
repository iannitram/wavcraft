//! This process's memory: the most it has held since it started, and what it holds now.
//!
//! Windows: `PeakWorkingSetSize` and `WorkingSetSize` from `GetProcessMemoryInfo`. Linux: `VmHWM`
//! and `VmRSS` from `/proc/self/status`. Elsewhere: not known.

/// Bytes, when the platform says.
#[derive(Debug, Clone, Copy, Default)]
pub struct Memory {
    /// The largest resident set (working set) so far.
    pub peak: Option<u64>,
    /// The resident set (working set) now.
    pub now: Option<u64>,
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn read() -> Memory {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0);
    let mut counters = PROCESS_MEMORY_COUNTERS { cb: size, ..Default::default() };
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always valid and needs no
    // closing. `counters` is a live, writable `PROCESS_MEMORY_COUNTERS`, and `size` is its size,
    // which is what the call writes at most.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    if ok == 0 {
        return Memory::default();
    }
    Memory { peak: Some(counters.PeakWorkingSetSize as u64), now: Some(counters.WorkingSetSize as u64) }
}

#[cfg(target_os = "linux")]
pub fn read() -> Memory {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else { return Memory::default() };
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    Memory { peak: field("VmHWM:"), now: field("VmRSS:") }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn read() -> Memory {
    Memory::default()
}
