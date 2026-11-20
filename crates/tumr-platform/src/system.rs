//! Host hardware and hypervisor facts (ENV-001, ENV-007).

use std::io;
use std::path::Path;

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::{
    GetNativeSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, PROCESSOR_ARCHITECTURE_AMD64,
    PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL, SYSTEM_INFO,
};
use windows_sys::Win32::System::Threading::IsProcessorFeaturePresent;

use crate::wide::to_wide;

/// Physical memory in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Physical memory of the machine.
pub fn memory() -> io::Result<Memory> {
    // SAFETY: plain-data struct; dwLength is set before the call as required.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).unwrap_or(0);
    // SAFETY: `m` is a valid, correctly sized MEMORYSTATUSEX.
    if unsafe { GlobalMemoryStatusEx(&raw mut m) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Memory {
        total_bytes: m.ullTotalPhys,
        available_bytes: m.ullAvailPhys,
    })
}

/// Disk space of the volume holding `path` (the path must exist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    /// Free bytes available to the current user (respects quotas).
    pub free_bytes: u64,
    pub total_bytes: u64,
}

pub fn disk_space(path: &Path) -> io::Result<DiskSpace> {
    let w = to_wide(path.as_os_str());
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: NUL-terminated path; three valid out-pointers.
    if unsafe {
        GetDiskFreeSpaceExW(
            w.as_ptr(),
            &raw mut free,
            &raw mut total,
            &raw mut total_free,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(DiskSpace {
        free_bytes: free,
        total_bytes: total,
    })
}

/// Native CPU architecture of the machine (not of this process): `x86_64`, `aarch64`, `x86`, or `unknown`.
#[must_use]
pub fn native_arch() -> &'static str {
    // SAFETY: plain-data out struct, filled by the call.
    let mut si: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: `si` is a valid out-pointer.
    unsafe { GetNativeSystemInfo(&raw mut si) };
    // SAFETY: reading the architecture field of the union written by GetNativeSystemInfo.
    let arch = unsafe { si.Anonymous.Anonymous.wProcessorArchitecture };
    match arch {
        PROCESSOR_ARCHITECTURE_AMD64 => "x86_64",
        PROCESSOR_ARCHITECTURE_ARM64 => "aarch64",
        PROCESSOR_ARCHITECTURE_INTEL => "x86",
        _ => "unknown",
    }
}

/// `PF_VIRT_FIRMWARE_ENABLED`: VT-x / AMD-V enabled in firmware.
///
/// Note (POC-000): Windows reports **false** while a hypervisor (Hyper-V/WHPX) is already
/// running, because the CPU feature is then owned by the hypervisor. Combine with
/// [`hypervisor_present`] and [`whpx_status`].
#[must_use]
pub fn virtualization_firmware_enabled() -> bool {
    const PF_VIRT_FIRMWARE_ENABLED: u32 = 21;
    // SAFETY: no preconditions.
    unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) != 0 }
}

/// Whether this OS runs on top of a hypervisor (CPUID leaf 1, ECX bit 31).
#[must_use]
pub fn hypervisor_present() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        #[allow(unused_unsafe)]
        // SAFETY: CPUID is always available on x86_64.
        let r = unsafe { std::arch::x86_64::__cpuid(1) };
        (r.ecx >> 31) & 1 == 1
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Windows Hypervisor Platform (the emulator's preferred accelerator) state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhpxStatus {
    /// API present and reports a running hypervisor: the emulator can use it.
    Usable,
    /// `WinHvPlatform.dll` exists but no hypervisor is running (feature on, but not active — reboot or firmware setting).
    NoHypervisor,
    /// The optional feature "Windows Hypervisor Platform" is not installed.
    NotInstalled,
    /// The capability query failed.
    Error(String),
}

/// Asks `WinHvPlatform.dll` (loaded dynamically, so a missing feature is not a crash) whether
/// a hypervisor is present.
#[must_use]
pub fn whpx_status() -> WhpxStatus {
    type WHvGetCapability =
        unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
    const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 = 0;

    let name = to_wide("WinHvPlatform.dll");
    // SAFETY: NUL-terminated module name; the result is checked for null.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return WhpxStatus::NotInstalled;
    }
    // SAFETY: valid module handle and NUL-terminated ANSI symbol name.
    let proc = unsafe { GetProcAddress(module, c"WHvGetCapability".as_ptr().cast()) };
    let status = match proc {
        None => WhpxStatus::Error("WHvGetCapability not exported".into()),
        Some(p) => {
            // SAFETY: the export has exactly this documented signature.
            let f: WHvGetCapability = unsafe { std::mem::transmute(p) };
            let mut present: i32 = 0;
            let mut written = 0u32;
            // SAFETY: output buffer is a 4-byte BOOL as required for this capability code.
            let hr = unsafe {
                f(
                    WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT,
                    (&raw mut present).cast(),
                    4,
                    &raw mut written,
                )
            };
            if hr < 0 {
                WhpxStatus::Error(format!("WHvGetCapability failed: 0x{hr:08x}"))
            } else if present != 0 {
                WhpxStatus::Usable
            } else {
                WhpxStatus::NoHypervisor
            }
        }
    };
    // SAFETY: releases the reference taken by LoadLibraryW.
    unsafe { FreeLibrary(module) };
    status
}

/// Whether the Android Emulator Hypervisor Driver (AEHD) service is installed.
#[must_use]
pub fn aehd_installed() -> bool {
    crate::registry::read_string(
        crate::registry::Hive::LocalMachine,
        r"SYSTEM\CurrentControlSet\Services\aehd",
        "ImagePath",
    )
    .ok()
    .flatten()
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_plausible() {
        let m = memory().unwrap();
        assert!(m.total_bytes > 1 << 30, "{m:?}");
        assert!(m.available_bytes <= m.total_bytes);
    }

    #[test]
    fn disk_space_of_temp_dir() {
        let d = disk_space(&std::env::temp_dir()).unwrap();
        assert!(d.total_bytes > 0 && d.free_bytes <= d.total_bytes, "{d:?}");
    }

    #[test]
    fn arch_is_known() {
        assert_ne!(native_arch(), "unknown");
    }

    #[test]
    fn virtualization_queries_do_not_crash() {
        let _ = virtualization_firmware_enabled();
        let _ = hypervisor_present();
        let _ = aehd_installed();
        match whpx_status() {
            WhpxStatus::Error(e) => panic!("WHPX query error: {e}"),
            s => eprintln!("WHPX: {s:?}"),
        }
    }
}
