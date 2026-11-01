//! Keeping the sign-in token safe. On Windows it's kept by Windows' Credential Manager, for this
//! Windows user only; account.json then just says so. Elsewhere the file it's in is readable by
//! its owner only.

/// Where account.json says the token is kept.
#[cfg(windows)]
const IN_WINDOWS: &str = "credential-manager";

/// Puts the token away: what account.json keeps instead of it. None: it couldn't be (it's then
/// kept as it is, in a file only this user can read).
pub fn seal(plain: &str) -> Option<String> {
    #[cfg(windows)]
    {
        crate::win::cred_write(plain).then(|| IN_WINDOWS.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = plain;
        None
    }
}

/// The token again.
pub fn open(sealed: &str) -> Option<String> {
    #[cfg(windows)]
    {
        if sealed == IN_WINDOWS {
            return crate::win::cred_read();
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = sealed;
        None
    }
}

/// Signed out: the token kept by Windows goes too.
pub fn forget() {
    #[cfg(windows)]
    crate::win::cred_delete();
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
