//! Host-environment facts the app reports but does not bundle (D-162).

/// True when the Microsoft Visual C++ 2015-2022 x64 runtime is loadable.
///
/// The bundled `llama-server.exe` imports `vcruntime140.dll`,
/// `vcruntime140_1.dll` and `msvcp140.dll`. They are NOT shipped in the
/// installer, so on a clean machine the engine fails to start with no useful
/// message. Loading the DLLs into this process is the only reliable probe.
#[cfg(windows)]
pub fn msvc_runtime_present() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(
            name: *const u16,
            file: *mut core::ffi::c_void,
            flags: u32,
        ) -> *mut core::ffi::c_void;
        fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
    }

    // Restrict resolution to System32 so a stray DLL in the app/CWD can't
    // hijack the probe and the result reflects the system-installed redist.
    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;

    fn loads(dll: &str) -> bool {
        let mut wide: Vec<u16> = dll.encode_utf16().collect();
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the
        // call; a null handle is checked before FreeLibrary.
        unsafe {
            let handle = LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if handle.is_null() {
                return false;
            }
            FreeLibrary(handle);
            true
        }
    }

    loads("vcruntime140.dll") && loads("vcruntime140_1.dll") && loads("msvcp140.dll")
}

#[cfg(not(windows))]
pub fn msvc_runtime_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_deterministic() {
        let first = msvc_runtime_present();
        assert_eq!(first, msvc_runtime_present());
    }
}
