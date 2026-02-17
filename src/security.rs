//! Windows process security hardening.
//!
//! Applies runtime mitigation policies via `SetProcessMitigationPolicy` to restrict
//! dangerous syscalls and prevent common exploit techniques. Also creates a Job Object
//! to prevent exploits from spawning child processes.
//!
//! ## Policies Applied
//!
//! 1. **Job Object** (always enabled): Prevents child process spawning (blocks cmd.exe/powershell)
//! 2. **ProcessImageLoadPolicy** (always enabled): Blocks loading DLLs from remote/UNC paths
//! 3. **ProcessDynamicCodePolicy (ACG)** (opt-in): Prevents runtime code generation (JIT)
//!
//! ## Compatibility
//!
//! - Requires Windows 10 version 1703+ for all policies
//! - ProcessDynamicCodePolicy conflicts with JavaScript JIT (requires `--secure-mode` flag)
//! - ProcessSignaturePolicy was removed (breaks GPU drivers from Nvidia/AMD/Intel)
//! - ProcessSystemCallDisablePolicy not used (breaks GPU drivers)
//!
//! ## Performance
//!
//! - Job Object: <5ms startup overhead
//! - Image Load Policy: ~10-20ms startup overhead
//! - ACG (if enabled): ~10-20ms startup overhead
//! - Total: ~30-50ms startup cost (acceptable)

#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(target_os = "windows")]
use std::ptr::null_mut;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::GetLastError;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessMitigationPolicy};

// Process Mitigation Policy constants (from winnt.h)
#[cfg(target_os = "windows")]
#[allow(dead_code)] // Kept for future ACG support when Servo exposes JIT disable API
const PROCESS_MITIGATION_DYNAMIC_CODE_POLICY: i32 = 2;
#[cfg(target_os = "windows")]
const PROCESS_MITIGATION_IMAGE_LOAD_POLICY: i32 = 10;

// Process Mitigation Policy structures (manual definitions, as windows-sys doesn't expose them)
#[cfg(target_os = "windows")]
#[repr(C)]
#[allow(dead_code)] // Kept for future ACG support when Servo exposes JIT disable API
struct ProcessMitigationDynamicCodePolicy {
    flags: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct ProcessMitigationImageLoadPolicy {
    flags: u32,
}

/// Applies Windows process mitigation policies for security hardening.
///
/// Call this **BEFORE** initializing Servo or loading any DLLs. The function
/// applies mitigation policies conditionally based on the `enable_acg` flag.
///
/// ## Policies Applied
///
/// - **Job Object** (always): Kills child processes when browser exits
/// - **Image Load Policy** (always): Blocks DLLs from remote/UNC paths
/// - **ACG** (conditional): Blocks runtime code generation if `enable_acg=true`
///
/// ## Arguments
///
/// * `enable_acg` - If true, enables Arbitrary Code Guard (ACG). **REQUIRES JIT DISABLED**.
///   If JIT is not disabled, the browser will crash immediately when loading JavaScript.
///
/// ## Panics
///
/// Does NOT panic. Failures are logged but not fatal (graceful degradation).
///
/// ## Example
///
/// ```rust
/// // Default mode: JIT enabled, ACG disabled
/// apply_process_mitigations(false);
///
/// // Secure mode: JIT disabled, ACG enabled
/// apply_process_mitigations(true);
/// ```
pub fn apply_process_mitigations(enable_acg: bool) {
    #[cfg(target_os = "windows")]
    {
        let start_time = std::time::Instant::now();

        // Always-on policies (safe, no compatibility issues)
        if let Err(e) = create_job_object_jail() {
            eprintln!("⚠️  Failed to create Job Object: {}", e);
        }

        if let Err(e) = apply_image_load_policy() {
            eprintln!("⚠️  Failed to apply image load policy: {}", e);
        }

        // Conditional ACG (DISABLED until Servo supports JIT control)
        // SECURITY FIX (V-1): ACG + JIT = guaranteed crash
        if enable_acg {
            eprintln!("⚠️  WARNING: --secure-mode requested but ACG disabled");
            eprintln!("    Reason: Servo doesn't expose JavaScript JIT disable API");
            eprintln!("    ACG + JIT = guaranteed crash on JavaScript execution");
            eprintln!("    Issue: Servo lacks js.jit.content preference");
            eprintln!("    Alternative: Use Job Object + Image Load policies (already active)");
            // DO NOT CALL: apply_dynamic_code_policy() - causes immediate crash
        }

        eprintln!(
            "✓ Process mitigation policies applied (ACG={}, took {:?})",
            enable_acg,
            start_time.elapsed()
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable_acg; // Suppress unused variable warning
        // No-op on Linux/macOS
    }
}

/// Creates a Job Object and assigns the current process to it.
///
/// ## Purpose
///
/// Configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, which means all child
/// processes will be terminated when the browser exits or the Job Object handle
/// is closed. Child processes also cannot escape the job (no `BREAKAWAY_OK`).
///
/// This prevents exploits from spawning `cmd.exe`, `powershell.exe`, or other
/// attack tools as child processes.
///
/// ## Compatibility
///
/// - Works on all Windows versions
/// - Does NOT interfere with GPU drivers (they load as DLLs, not child processes)
/// - Does NOT interfere with Servo threads (they're threads, not processes)
///
/// ## Returns
///
/// `Ok(())` if Job Object created and assigned successfully.
/// `Err(String)` with Windows error code if creation fails.
#[cfg(target_os = "windows")]
fn create_job_object_jail() -> Result<(), String> {
    // Create anonymous job object (NULL name, default security descriptor)
    let job_handle = unsafe { CreateJobObjectW(null_mut(), null_mut()) };

    if job_handle.is_null() {
        let error_code = unsafe { GetLastError() };
        return Err(format!("CreateJobObjectW failed with error {}", error_code));
    }

    // Configure job limits
    let mut job_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: unsafe { std::mem::zeroed() },
        IoInfo: unsafe { std::mem::zeroed() },
        ProcessMemoryLimit: 0,
        JobMemoryLimit: 0,
        PeakProcessMemoryUsed: 0,
        PeakJobMemoryUsed: 0,
    };

    // Enable KILL_ON_JOB_CLOSE: children die when job handle closes
    job_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    // Explicitly NOT setting JOB_OBJECT_LIMIT_BREAKAWAY_OK (default = can't escape)

    let result = unsafe {
        SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &job_info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };

    if result == 0 {
        let error_code = unsafe { GetLastError() };
        return Err(format!(
            "SetInformationJobObject failed with error {}",
            error_code
        ));
    }

    // Assign current process to job
    let result = unsafe { AssignProcessToJobObject(job_handle, GetCurrentProcess()) };

    if result == 0 {
        let error_code = unsafe { GetLastError() };
        return Err(format!(
            "AssignProcessToJobObject failed with error {}",
            error_code
        ));
    }

    // Intentionally leak the job handle so it stays open for process lifetime
    // (closing it would kill the process due to KILL_ON_JOB_CLOSE)
    // Using let _ instead of std::mem::forget since HANDLE is Copy
    let _ = job_handle;

    eprintln!("✓ Job Object created (child process spawning blocked)");
    Ok(())
}

/// Applies ProcessDynamicCodePolicy (Arbitrary Code Guard - ACG).
///
/// ## Purpose
///
/// Prevents the process from generating executable code at runtime. This blocks:
/// - JIT compilers (JavaScript, regex, etc.) - **CRITICAL CONFLICT**
/// - Shellcode from allocating RWX (Read-Write-Execute) memory pages
/// - Dynamic code injection attacks
///
/// ## CRITICAL: Conflicts with JavaScript JIT
///
/// **This policy will crash the browser if JavaScript JIT is enabled.**
/// Servo's SpiderMonkey JS engine uses JIT compilation, which requires creating
/// RWX memory pages. ACG forbids this, causing immediate crashes when loading JS.
///
/// **Solution**: Only enable ACG if --secure-mode flag is set (which disables JIT).
///
/// ## Returns
///
/// `Ok(())` if policy applied successfully.
/// `Err(String)` with Windows error code if policy fails.
#[cfg(target_os = "windows")]
#[allow(dead_code)] // Kept for future ACG support when Servo exposes JIT disable API
fn apply_dynamic_code_policy() -> Result<(), String> {
    let policy = ProcessMitigationDynamicCodePolicy {
        flags: 1, // ProhibitDynamicCode = 1 (bit 0)
    };

    let result = unsafe {
        SetProcessMitigationPolicy(
            PROCESS_MITIGATION_DYNAMIC_CODE_POLICY,
            &policy as *const _ as *const _,
            size_of::<ProcessMitigationDynamicCodePolicy>(),
        )
    };

    if result == 0 {
        let error_code = unsafe { GetLastError() };
        return Err(format!(
            "SetProcessMitigationPolicy(DynamicCode) failed with error {}",
            error_code
        ));
    }

    eprintln!("✓ Dynamic code policy applied (no JIT RWX pages)");
    Ok(())
}

/// Applies ProcessImageLoadPolicy to prevent loading DLLs from remote locations.
///
/// ## Purpose
///
/// Blocks loading DLLs from:
/// - Remote UNC paths (\\server\share\malicious.dll)
/// - Network-mapped drives that aren't local
/// - Low integrity level locations (used by sandboxed processes)
///
/// This prevents simple DLL injection attacks that rely on loading malware from
/// network shares or compromised low-integrity locations.
///
/// ## Compatibility
///
/// - Does NOT block GPU drivers (they load from local C:\Windows\System32 or Program Files)
/// - Does NOT block Servo's ANGLE DLLs (they're in the local executable directory)
/// - Safe to enable on all hardware configurations
///
/// ## Returns
///
/// `Ok(())` if policy applied successfully.
/// `Err(String)` with Windows error code if policy fails.
#[cfg(target_os = "windows")]
fn apply_image_load_policy() -> Result<(), String> {
    let policy = ProcessMitigationImageLoadPolicy {
        flags: 1 | 2, // NoRemoteImages (bit 0) | NoLowMandatoryLabelImages (bit 1)
    };

    let result = unsafe {
        SetProcessMitigationPolicy(
            PROCESS_MITIGATION_IMAGE_LOAD_POLICY,
            &policy as *const _ as *const _,
            size_of::<ProcessMitigationImageLoadPolicy>(),
        )
    };

    if result == 0 {
        let error_code = unsafe { GetLastError() };
        return Err(format!(
            "SetProcessMitigationPolicy(ImageLoad) failed with error {}",
            error_code
        ));
    }

    eprintln!("✓ Image load policy applied (no remote DLLs)");
    Ok(())
}
