//! Keeping the sign-in token safe. On Windows it's kept by Windows' Credential Manager, for this
//! Windows user only; account.json then just says so. Elsewhere the file it's in is readable by
//! its owner only.

/// Separate providers must never replace or remove each other's credentials.
pub fn save_named(dir: &std::path::Path, name: &str, plain: &[u8]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let _ = dir;
        let text = std::str::from_utf8(plain).map_err(std::io::Error::other)?;
        if !crate::win::cred_write_named(&format!("RecompLauncher/{name}"), text) {
            return Err(std::io::Error::other("Couldn't save the Google sign-in in Credential Manager."));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let path = dir.join(format!("{name}.json"));
        let temp = dir.join(format!("{name}.json.tmp"));
        write_private(&temp, plain)?;
        std::fs::rename(temp, path)
    }
}

pub fn load_named(dir: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        let _ = dir;
        crate::win::cred_read_named(&format!("RecompLauncher/{name}")).map(String::into_bytes)
    }
    #[cfg(not(windows))]
    std::fs::read(dir.join(format!("{name}.json"))).ok()
}

pub fn forget_named(dir: &std::path::Path, name: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let _ = dir;
        if !crate::win::cred_delete_named(&format!("RecompLauncher/{name}")) {
            return Err(std::io::Error::other("Couldn't remove the Google sign-in from Credential Manager."));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    match std::fs::remove_file(dir.join(format!("{name}.json"))) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

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
