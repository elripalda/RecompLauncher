//! Keeping the sign-in token safe on disk. On Windows it's sealed with DPAPI: only the same Windows
//! user on the same PC can open it again. Elsewhere the file it's in is readable by its owner only.

/// The token sealed for this user, as text for account.json. None: it couldn't be (it's then kept
/// as it is, in a file only this user can read).
pub fn seal(plain: &str) -> Option<String> {
    #[cfg(windows)]
    {
        dpapi::protect(plain.as_bytes()).map(|b| hex::encode(b))
    }
    #[cfg(not(windows))]
    {
        let _ = plain;
        None
    }
}

/// A sealed token opened again.
pub fn open(sealed: &str) -> Option<String> {
    #[cfg(windows)]
    {
        hex::decode(sealed).ok().and_then(|b| dpapi::unprotect(&b)).and_then(|b| String::from_utf8(b).ok())
    }
    #[cfg(not(windows))]
    {
        let _ = sealed;
        None
    }
}

/// Writes a file only its owner can read (on Windows the folder's own permissions already do that).
pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(bytes)?;
        // A file made before this keeps its old permissions: set them too.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

#[cfg(windows)]
mod dpapi {
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(input: *const Blob, desc: *const u16, entropy: *const Blob, reserved: *const u8, prompt: *const u8, flags: u32, out: *mut Blob) -> i32;
        fn CryptUnprotectData(input: *const Blob, desc: *mut *mut u16, entropy: *const Blob, reserved: *const u8, prompt: *const u8, flags: u32, out: *mut Blob) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(mem: *mut u8) -> *mut u8;
    }
    // No pop-ups, ever.
    const UI_FORBIDDEN: u32 = 0x1;
    const ENTROPY: &[u8] = b"RecompLauncher account";

    fn run(input: &[u8], seal: bool) -> Option<Vec<u8>> {
        let inb = Blob { len: input.len() as u32, data: input.as_ptr() as *mut u8 };
        let ent = Blob { len: ENTROPY.len() as u32, data: ENTROPY.as_ptr() as *mut u8 };
        let mut out = Blob { len: 0, data: std::ptr::null_mut() };
        let ok = unsafe {
            if seal {
                CryptProtectData(&inb, std::ptr::null(), &ent, std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&inb, std::ptr::null_mut(), &ent, std::ptr::null(), std::ptr::null(), UI_FORBIDDEN, &mut out)
            }
        };
        if ok == 0 || out.data.is_null() {
            return None;
        }
        let v = unsafe { std::slice::from_raw_parts(out.data, out.len as usize).to_vec() };
        unsafe { LocalFree(out.data) };
        Some(v)
    }
    pub fn protect(b: &[u8]) -> Option<Vec<u8>> {
        run(b, true)
    }
    pub fn unprotect(b: &[u8]) -> Option<Vec<u8>> {
        run(b, false)
    }
}
