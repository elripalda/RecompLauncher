//! Files only the person running the launcher can read. The control API's endpoint file holds its
//! token, so it's written here and nowhere else.
//!
//! What "private" takes, and why:
//! - The folder and the file belong to this user (an owner can always change who may open
//!   something), and nobody else is let in. Unix: mode 0700/0600, and on macOS no access list
//!   entries either (macOS keeps those apart from the mode, and a folder above hands them down to
//!   new files). Windows: a protected access list with one entry, full access for this user.
//! - The file is checked through the handle it's written through, while it's still empty: its
//!   name can't be swapped for another file between the check and the write.
//! - No other user can rename or replace the folders it's in, or any link on the way to it as the
//!   launcher and scripts name it. One who can could put their own folder (and their own endpoint
//!   file) in its place, so a shared or portable place where they can (a folder anyone or its
//!   group may change, a drive without access lists, a link in a shared folder) is refused.
//! - Windows: the folder is made private in the same step that makes it. One that was there
//!   already and isn't private is refused, not repaired: on Windows, someone who opened it while
//!   they were allowed keeps what they opened.
//!
//! Anything that can't be made private is refused rather than written: the API then stays off.
//! This file only uses std and the `windows` crate, so it can be compile-checked on its own.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// `dir` made if needed, then limited to this user. Its parent folder has to exist already.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    let parent = dir.parent().ok_or_else(|| io::Error::other("it has no folder above it"))?;
    // Checked first: in a folder others can change, making this one private wouldn't be enough.
    imp::nobody_else_can_replace(parent)?;
    imp::make_dir(dir)
}

/// `bytes` in `path` (inside a [`private_dir`]), readable by this user only. Written to a new
/// file that is private from the moment it exists, checked, then moved into place. On any failure
/// nothing is left behind with the bytes in it.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = tmp_path(path);
    // A copy left by a launcher that stopped mid-write: it's in the private folder, so it's ours.
    let _ = std::fs::remove_file(&tmp);
    let written = write_then_move(&tmp, path, bytes);
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(path);
    }
    written
}

/// Ok when only this user can open `path` (a file or a folder), and it's this user's.
pub fn is_private(path: &Path) -> io::Result<()> {
    imp::is_private(path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    path.with_file_name(format!("{name}.tmp-{}", std::process::id()))
}

fn write_then_move(tmp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = imp::create_private(tmp)?;
    // Checked while it's still empty, through the handle that writes it.
    imp::check_file(&f)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)?;
    // The name now leads to the file written, and it's still private.
    imp::same_file(&f, path)?;
    imp::check_file(&f)
}

#[cfg(unix)]
mod imp {
    use super::acl;
    use std::ffi::OsString;
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Component, Path, PathBuf};

    extern "C" {
        fn geteuid() -> u32;
    }

    pub(super) fn me() -> u32 {
        unsafe { geteuid() }
    }

    pub fn make_dir(dir: &Path) -> io::Result<()> {
        // 0700 from the start (the umask can only take bits away).
        if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir) {
            if e.kind() != io::ErrorKind::AlreadyExists {
                return Err(e);
            }
        }
        let d = open_no_link(dir)?;
        let meta = d.metadata()?;
        if !meta.is_dir() {
            return Err(io::Error::other("not a plain folder"));
        }
        // Someone else's folder: this user can't make it private, and its owner could undo it.
        if meta.uid() != me() {
            return Err(io::Error::other("it belongs to another user"));
        }
        // This user's own: Unix checks access each time something in it is opened, so from here on
        // only this user gets in, whatever it allowed before.
        d.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        acl::clear(&d)?;
        check(&d, true)
    }

    pub fn create_private(path: &Path) -> io::Result<File> {
        // create_new never follows a link, and the umask can only take bits away from 0600.
        let f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
        // macOS: entries a folder above handed down to it go.
        acl::clear(&f)?;
        Ok(f)
    }

    pub fn check_file(f: &File) -> io::Result<()> {
        check(f, false)
    }

    pub fn same_file(f: &File, path: &Path) -> io::Result<()> {
        let held = f.metadata()?;
        let named = std::fs::symlink_metadata(path)?;
        if (held.dev(), held.ino()) != (named.dev(), named.ino()) {
            return Err(io::Error::other("it was replaced while it was written"));
        }
        Ok(())
    }

    pub fn is_private(path: &Path) -> io::Result<()> {
        let f = open_no_link(path)?;
        let dir = f.metadata()?.is_dir();
        check(&f, dir)
    }

    fn check(f: &File, dir: bool) -> io::Result<()> {
        let meta = f.metadata()?;
        if meta.is_dir() != dir || !(meta.is_dir() || meta.is_file()) {
            return Err(io::Error::other(if dir { "not a plain folder" } else { "not a plain file" }));
        }
        if meta.uid() != me() {
            return Err(io::Error::other("it belongs to another user"));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(io::Error::other("others can open it"));
        }
        acl::none_left(f)
    }

    /// `path` itself, opened to look at it. Refused when it's a link, or something that isn't a
    /// file or a folder (opening a pipe would wait for ever).
    fn open_no_link(path: &Path) -> io::Result<File> {
        let named = std::fs::symlink_metadata(path)?;
        if named.file_type().is_symlink() {
            return Err(io::Error::other("it's a link"));
        }
        if !(named.is_dir() || named.is_file()) {
            return Err(io::Error::other("not a plain file or folder"));
        }
        let f = File::open(path)?;
        let meta = f.metadata()?;
        if (meta.dev(), meta.ino()) != (named.dev(), named.ino()) {
            return Err(io::Error::other("it changed while it was checked"));
        }
        Ok(f)
    }

    /// Why another user could rename or replace what's in a folder with this owner and mode, or
    /// (for a link) the link itself; None when they can't. Only this user and root are trusted as
    /// owners: an owner can always change the rest. Write for the folder's group is refused even
    /// when the group is this user's own: the number alone doesn't say who else is in it, and on
    /// Linux a named user's access list entry shows up as group write too. A sticky folder (like
    /// /tmp) is fine, since only the owner of something in it may rename that, and each thing on
    /// the way is checked to be this user's or root's.
    pub(super) fn others_can_replace(uid: u32, mode: u32, is_dir: bool, me: u32) -> Option<&'static str> {
        if uid != me && uid != 0 {
            return Some("it belongs to another user");
        }
        // A link's own mode means nothing; who may replace it is up to its folder.
        if !is_dir || mode & 0o1000 != 0 {
            return None;
        }
        if mode & 0o002 != 0 {
            return Some("anyone can change it");
        }
        if mode & 0o020 != 0 {
            return Some("its group can change it");
        }
        None
    }

    /// The most links followed on the way, as Linux's own limit: a loop of links ends here.
    const MAX_LINKS: usize = 40;

    /// Err when another user could rename or replace `dir` or anything on the way to it, as it's
    /// named: every folder from the top, every link on the way (and the folders its target goes
    /// through), and the folder it leads to. Checking only where it leads isn't enough: the
    /// launcher and scripts keep using the name, and a link in a folder others may change can be
    /// pointed somewhere else later. So /var on macOS (root's link to /private/var) is fine, and a
    /// link in a shared folder isn't, wherever it points.
    pub fn nobody_else_can_replace(dir: &Path) -> io::Result<()> {
        let named = if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir()?.join(dir) };
        // What's still to walk, next last; a link's target goes in its place.
        let mut todo: Vec<OsString> = vec![];
        push_route(&mut todo, &named);
        let mut at = PathBuf::from("/");
        check_place(&at)?;
        let mut links = 0;
        while let Some(name) = todo.pop() {
            if name == "/" {
                at = PathBuf::from("/");
                continue;
            }
            // `at` is a real folder reached from the top, all of it checked, so its parent is too.
            if name == ".." {
                at.pop();
                continue;
            }
            let next = at.join(&name);
            let meta = check_place(&next)?;
            if meta.file_type().is_symlink() {
                links += 1;
                if links > MAX_LINKS {
                    return Err(io::Error::other(format!("too many links on the way to {}", dir.display())));
                }
                push_route(&mut todo, &std::fs::read_link(&next)?);
                continue;
            }
            if !meta.is_dir() {
                return Err(io::Error::other(format!("{} isn't a folder", next.display())));
            }
            at = next;
        }
        Ok(())
    }

    /// `route`'s parts onto `todo`, to be walked first: "/" for a route from the top, ".." as it
    /// is. "." is nothing.
    fn push_route(todo: &mut Vec<OsString>, route: &Path) {
        for part in route.components().rev() {
            match part {
                Component::RootDir => todo.push("/".into()),
                Component::ParentDir => todo.push("..".into()),
                Component::Normal(n) => todo.push(n.to_os_string()),
                Component::CurDir | Component::Prefix(_) => {}
            }
        }
    }

    /// `place` itself (a link isn't followed), when no other user could replace what's in it, or
    /// it.
    fn check_place(place: &Path) -> io::Result<std::fs::Metadata> {
        let meta = std::fs::symlink_metadata(place)?;
        let unsafe_place = |why: &str| io::Error::other(format!("other users could replace {} ({why})", place.display()));
        if let Some(why) = others_can_replace(meta.uid(), meta.mode(), meta.is_dir(), me()) {
            return Err(unsafe_place(why));
        }
        acl::nobody_else_can_change(place).map_err(|e| unsafe_place(&e.to_string()))?;
        Ok(meta)
    }
}

/// macOS keeps access list entries apart from the mode: chmod 0600 leaves them in place, and a
/// folder above can hand them down to each new file in it. These are the few calls needed, from
/// the system library.
#[cfg(target_os = "macos")]
mod acl {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::fs::File;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    type Acl = *mut c_void;
    const ACL_TYPE_EXTENDED: c_int = 0x100;
    const ACL_FIRST_ENTRY: c_int = 0;
    const ACL_NEXT_ENTRY: c_int = -1;
    const ACL_EXTENDED_ALLOW: c_int = 1;
    /// Rights that let someone rename or replace what's in a folder (or the folder), or change
    /// who may.
    const REPLACE: [c_int; 4] = [1 << 4, 1 << 6, 1 << 12, 1 << 13]; // DELETE, DELETE_CHILD, WRITE_SECURITY, CHANGE_OWNER
    const ENOENT: i32 = 2;
    const ENOTSUP: i32 = 45;

    extern "C" {
        fn acl_init(count: c_int) -> Acl;
        fn acl_free(obj: *mut c_void) -> c_int;
        fn acl_get_fd_np(fd: c_int, kind: c_int) -> Acl;
        fn acl_set_fd_np(fd: c_int, acl: Acl, kind: c_int) -> c_int;
        fn acl_get_link_np(path: *const c_char, kind: c_int) -> Acl;
        fn acl_get_entry(acl: Acl, id: c_int, entry: *mut *mut c_void) -> c_int;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
        fn acl_get_permset(entry: *mut c_void, perms: *mut *mut c_void) -> c_int;
        fn acl_get_perm_np(perms: *mut c_void, perm: c_int) -> c_int;
    }

    /// An access list from the system, freed when dropped.
    struct List(Acl);

    impl Drop for List {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { acl_free(self.0) };
            }
        }
    }

    impl List {
        /// Each entry's kind (allow or deny) and rights.
        fn entries(&self) -> io::Result<Vec<(c_int, *mut c_void)>> {
            let mut out = vec![];
            let mut id = ACL_FIRST_ENTRY;
            let mut entry: *mut c_void = std::ptr::null_mut();
            while unsafe { acl_get_entry(self.0, id, &mut entry) } == 0 {
                id = ACL_NEXT_ENTRY;
                let mut tag: c_int = 0;
                let mut perms: *mut c_void = std::ptr::null_mut();
                if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || unsafe { acl_get_permset(entry, &mut perms) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                out.push((tag, perms));
            }
            Ok(out)
        }
    }

    /// No list (the usual answer) is Ok(None); so is a drive that can't keep one.
    fn got(acl: Acl) -> io::Result<Option<List>> {
        if !acl.is_null() {
            return Ok(Some(List(acl)));
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(ENOENT) | Some(ENOTSUP) => Ok(None),
            _ => Err(e),
        }
    }

    /// Every entry on an open file or folder goes.
    pub fn clear(f: &File) -> io::Result<()> {
        let empty = List(unsafe { acl_init(0) });
        if empty.0.is_null() {
            return Err(io::Error::last_os_error());
        }
        if unsafe { acl_set_fd_np(f.as_raw_fd(), empty.0, ACL_TYPE_EXTENDED) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        // A drive without access lists has none to clear (its modes are checked all the same).
        if e.raw_os_error() == Some(ENOTSUP) {
            return Ok(());
        }
        Err(e)
    }

    /// Ok when an open file or folder has no entries.
    pub fn none_left(f: &File) -> io::Result<()> {
        let Some(list) = got(unsafe { acl_get_fd_np(f.as_raw_fd(), ACL_TYPE_EXTENDED) })? else { return Ok(()) };
        if !list.entries()?.is_empty() {
            return Err(io::Error::other("others are in its access list"));
        }
        Ok(())
    }

    /// Err when an entry lets someone rename or replace what's in `place` (a folder), or `place`
    /// itself (a folder or a link, which isn't followed). Deny entries only take rights away, so
    /// they're fine.
    pub fn nobody_else_can_change(place: &Path) -> io::Result<()> {
        let c = CString::new(place.as_os_str().as_bytes())?;
        let Some(list) = got(unsafe { acl_get_link_np(c.as_ptr(), ACL_TYPE_EXTENDED) })? else { return Ok(()) };
        for (tag, perms) in list.entries()? {
            if tag != ACL_EXTENDED_ALLOW {
                continue;
            }
            if REPLACE.iter().any(|p| unsafe { acl_get_perm_np(perms, *p) } == 1) {
                return Err(io::Error::other("an access list entry lets others rename or replace what's in it"));
            }
        }
        Ok(())
    }
}

/// Linux and the other Unixes: access list entries for named users and groups there are capped by
/// the group bits of the mode (its mask). A mode with no group or other bits leaves them nothing,
/// and one whose group bits allow writing is refused on the way (see `others_can_replace`).
#[cfg(all(unix, not(target_os = "macos")))]
mod acl {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    pub fn clear(_: &File) -> io::Result<()> {
        Ok(())
    }

    pub fn none_left(_: &File) -> io::Result<()> {
        Ok(())
    }

    pub fn nobody_else_can_change(_: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use windows::core::{BOOL, HRESULT, HSTRING, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_ALL, HANDLE, HLOCAL, WIN32_ERROR};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetSecurityDescriptorControl, GetTokenInformation, TokenUser, ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
        INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, DELETE, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Accounts that run Windows itself. They (and this user) may own and change the folders
    /// above; administrators are trusted, as root is on Unix.
    const SYSTEM_ACCOUNTS: [&str; 4] = [
        "S-1-5-18",     // SYSTEM
        "S-1-5-32-544", // Administrators
        "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464", // TrustedInstaller
        "S-1-3-4",      // OWNER RIGHTS: whoever owns it, checked on its own
    ];
    /// ACCESS_ALLOWED_ACE_TYPE and ACCESS_ALLOWED_CALLBACK_ACE_TYPE: an entry laid out with its
    /// account right after its rights.
    const ALLOWED: [u8; 2] = [0, 9];
    /// The kinds of entry that only take rights away.
    const DENIED: [u8; 4] = [1, 6, 10, 12];

    fn err(what: &str, e: impl std::fmt::Display) -> io::Error {
        io::Error::other(format!("{what}: {e}"))
    }

    fn win32(what: &str, e: WIN32_ERROR) -> io::Result<()> {
        if e.0 == 0 {
            return Ok(());
        }
        Err(err(what, io::Error::from_raw_os_error(e.0 as i32)))
    }

    fn handle(f: &File) -> HANDLE {
        HANDLE(f.as_raw_handle())
    }

    /// Memory Windows handed back with LocalAlloc, freed when dropped.
    struct Local(*mut c_void);

    impl Drop for Local {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0)));
                }
            }
        }
    }

    fn sid_text(sid: PSID) -> io::Result<String> {
        unsafe {
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(sid, &mut text).map_err(|e| err("can't name an account", e))?;
            let _free = Local(text.0.cast());
            text.to_string().map_err(|e| err("can't name an account", e))
        }
    }

    /// The account this process runs as, as text (S-1-5-21-...).
    pub fn me() -> io::Result<String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| err("can't read this user", e))?;
            let mut len = 0u32;
            // The first call only says how big the answer is.
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            // u64s keep TOKEN_USER aligned.
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let read = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), (buf.len() * 8) as u32, &mut len);
            let _ = CloseHandle(token);
            read.map_err(|e| err("can't read this user", e))?;
            sid_text((*(buf.as_ptr() as *const TOKEN_USER)).User.Sid)
        }
    }

    /// A security descriptor owned by `me`, with a protected access list of one entry: full access
    /// for `me`. `inherit`: a folder's entry applies to what's made in it too.
    fn only_me(me: &str, inherit: bool) -> io::Result<Local> {
        let flags = if inherit { "OICI" } else { "" };
        let sddl = HSTRING::from(format!("O:{me}D:P(A;{flags};FA;;;{me})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None).map_err(|e| err("can't make the access list", e))?;
        }
        Ok(Local(sd.0))
    }

    fn attributes(sd: &Local) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: BOOL(0) }
    }

    pub fn make_dir(dir: &Path) -> io::Result<()> {
        let me = me()?;
        let sd = only_me(&me, true)?;
        let sa = attributes(&sd);
        // Private and this user's from the moment it exists.
        if let Err(e) = unsafe { CreateDirectoryW(&HSTRING::from(dir), Some(&sa)) } {
            if e.code() != HRESULT::from_win32(ERROR_ALREADY_EXISTS.0) {
                return Err(err("can't make the folder", e));
            }
        }
        // One that was there already has to be private already (see the top of this file).
        let d = open_plain(dir)?;
        check(&d, true, &me).map_err(|e| io::Error::other(format!("{e}; delete the folder and try again")))
    }

    pub fn create_private(path: &Path) -> io::Result<File> {
        let me = me()?;
        let sd = only_me(&me, false)?;
        let sa = attributes(&sd);
        // Nobody may open it alongside, except to rename it (which takes the right to delete it,
        // and only this user has that): the launcher moves it into place while it's open.
        let access = FILE_GENERIC_WRITE.0 | READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
        let flags = FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT;
        let h = unsafe { CreateFileW(&HSTRING::from(path), access, FILE_SHARE_DELETE, Some(&sa), CREATE_NEW, flags, None) }.map_err(|e| err("can't make the file", e))?;
        Ok(unsafe { File::from_raw_handle(h.0) })
    }

    pub fn check_file(f: &File) -> io::Result<()> {
        check(f, false, &me()?)
    }

    pub fn same_file(f: &File, path: &Path) -> io::Result<()> {
        let named = open_plain(path)?;
        let (a, b) = (info(f)?, info(&named)?);
        if (a.dwVolumeSerialNumber, a.nFileIndexHigh, a.nFileIndexLow) != (b.dwVolumeSerialNumber, b.nFileIndexHigh, b.nFileIndexLow) {
            return Err(io::Error::other("it was replaced while it was written"));
        }
        Ok(())
    }

    pub fn is_private(path: &Path) -> io::Result<()> {
        let f = open_plain(path)?;
        let dir = info(&f)?.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        check(&f, dir, &me()?)
    }

    /// `path` itself (never where a link or junction leads), opened only to read its details and
    /// its access list.
    fn open_plain(path: &Path) -> io::Result<File> {
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
        let access = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
        let h = unsafe { CreateFileW(&HSTRING::from(path), access, share, None, OPEN_EXISTING, flags, None) }.map_err(|e| err("can't open it", e))?;
        Ok(unsafe { File::from_raw_handle(h.0) })
    }

    fn info(f: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(handle(f), &mut info) }.map_err(|e| err("can't read its details", e))?;
        Ok(info)
    }

    /// One entry of an access list. `who`: None for an entry whose account can't be read here.
    struct Entry {
        kind: u8,
        inherit_only: bool,
        mask: u32,
        who: Option<String>,
    }

    impl Entry {
        fn allows(&self) -> bool {
            !DENIED.contains(&self.kind)
        }
    }

    /// Who owns an open file or folder, whether its list is protected, and its entries (None: no
    /// list at all, which lets everyone in).
    struct Security {
        owner: String,
        protected: bool,
        entries: Option<Vec<Entry>>,
    }

    fn security(f: &File) -> io::Result<Security> {
        unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            let mut owner = PSID::default();
            let mut acl: *mut ACL = std::ptr::null_mut();
            let which = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
            win32("can't read its access list", GetSecurityInfo(handle(f), SE_FILE_OBJECT, which, Some(&mut owner), None, Some(&mut acl), None, Some(&mut sd)))?;
            let _free = Local(sd.0);
            let mut control = 0u16;
            let mut revision = 0u32;
            GetSecurityDescriptorControl(sd, &mut control, &mut revision).map_err(|e| err("can't read its access list", e))?;
            let entries = if acl.is_null() { None } else { Some(entries(acl)?) };
            Ok(Security { owner: sid_text(owner)?, protected: control & SE_DACL_PROTECTED.0 != 0, entries })
        }
    }

    fn entries(acl: *mut ACL) -> io::Result<Vec<Entry>> {
        let mut size = ACL_SIZE_INFORMATION::default();
        let len = std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32;
        unsafe { GetAclInformation(acl, (&mut size as *mut ACL_SIZE_INFORMATION).cast(), len, AclSizeInformation) }.map_err(|e| err("can't read its access list", e))?;
        let mut out = vec![];
        for i in 0..size.AceCount {
            let mut ace: *mut c_void = std::ptr::null_mut();
            unsafe { GetAce(acl, i, &mut ace) }.map_err(|e| err("can't read its access list", e))?;
            let ace = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let kind = ace.Header.AceType;
            let inherit_only = ace.Header.AceFlags as u32 & INHERIT_ONLY_ACE.0 != 0;
            // Denied entries have the same layout; the object kinds keep their account elsewhere.
            let laid_out = ALLOWED.contains(&kind) || kind == 1 || kind == 10;
            let who = if laid_out { Some(sid_text(PSID(&ace.SidStart as *const u32 as *mut c_void))?) } else { None };
            out.push(Entry { kind, inherit_only, mask: ace.Mask, who });
        }
        Ok(out)
    }

    /// Private: this user's, a plain file or folder (not a link or junction), with a protected
    /// list whose one entry gives this user full access to this file or folder.
    fn check(f: &File, dir: bool, me: &str) -> io::Result<()> {
        let attrs = info(f)?.dwFileAttributes;
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(io::Error::other("it's a link"));
        }
        if (attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != dir {
            return Err(io::Error::other(if dir { "not a plain folder" } else { "not a plain file" }));
        }
        let s = security(f)?;
        // Its owner can always change its list, whatever the list says now.
        if s.owner != me {
            return Err(io::Error::other("it belongs to another user"));
        }
        // Not protected: entries from the folders above it apply too.
        if !s.protected {
            return Err(io::Error::other("its access list takes entries from the folders above it"));
        }
        // No list at all (FAT drives, say) lets everyone in.
        let Some(list) = s.entries else { return Err(io::Error::other("this drive doesn't keep access lists")) };
        let [only] = list.as_slice() else { return Err(io::Error::other("others are in its access list")) };
        let full = only.mask == FILE_ALL_ACCESS.0 || only.mask & GENERIC_ALL.0 != 0;
        if !ALLOWED.contains(&only.kind) || only.inherit_only || !full || only.who.as_deref() != Some(me) {
            return Err(io::Error::other("its access list isn't only this user's"));
        }
        Ok(())
    }

    /// Err when another user could rename or replace `dir` or a folder above it, as it's named:
    /// one owned by another user, on a drive without access lists, or whose list lets others
    /// delete it, delete what's in it, or change the list or the owner. Portable folders on a USB
    /// drive, and folders made at the top of a drive (where every signed-in user may change them),
    /// are refused so. A link, junction or mounted folder on the way is refused too: whether
    /// others could change where it leads isn't worked out here.
    pub fn nobody_else_can_replace(dir: &Path) -> io::Result<()> {
        let me = me()?;
        let trusted = |who: &str| who == me || SYSTEM_ACCOUNTS.contains(&who);
        let replace = DELETE.0 | FILE_DELETE_CHILD.0 | WRITE_DAC.0 | WRITE_OWNER.0 | GENERIC_ALL.0;
        // Windows takes ".." away by name, before following anything, so this is the route taken.
        let named = std::path::absolute(dir)?;
        for d in named.ancestors() {
            let unsafe_place = |why: &str| io::Error::other(format!("other users could replace {} ({why})", d.display()));
            let f = open_plain(d)?;
            if info(&f)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err(unsafe_place("it's a link, junction or mounted folder"));
            }
            let s = security(&f)?;
            if !trusted(&s.owner) {
                return Err(unsafe_place("it belongs to another user"));
            }
            let Some(list) = s.entries else { return Err(unsafe_place("its drive doesn't keep access lists")) };
            let mut others = list.iter().filter(|e| e.allows() && !e.inherit_only && e.mask & replace != 0);
            if others.any(|e| !e.who.as_deref().map(trusted).unwrap_or(false)) {
                return Err(unsafe_place("others may delete or rename what's in it"));
            }
        }
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    fn no() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "files can't be made private on this system")
    }

    pub fn make_dir(_: &Path) -> io::Result<()> {
        Err(no())
    }

    pub fn create_private(_: &Path) -> io::Result<File> {
        Err(no())
    }

    pub fn check_file(_: &File) -> io::Result<()> {
        Err(no())
    }

    pub fn same_file(_: &File, _: &Path) -> io::Result<()> {
        Err(no())
    }

    pub fn is_private(_: &Path) -> io::Result<()> {
        Err(no())
    }

    pub fn nobody_else_can_replace(_: &Path) -> io::Result<()> {
        Err(no())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new folder for one test, in the system's temp folder (this user's, and safe).
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rl-private-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // Only this user's, whatever the umask: the tests open it up where they mean to.
        #[cfg(unix)]
        chmod(&d, 0o700);
        d
    }

    #[test]
    fn the_folder_and_the_file_are_private() {
        let d = temp("both").join("control-api");
        private_dir(&d).unwrap();
        assert!(is_private(&d).is_ok());
        let f = d.join("endpoint.json");
        write_private(&f, b"{\"token\":\"x\"}").unwrap();
        assert!(is_private(&f).is_ok());
        assert_eq!(std::fs::read(&f).unwrap(), b"{\"token\":\"x\"}");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn writing_again_replaces_it_and_leaves_no_temp_file() {
        let d = temp("again").join("control-api");
        private_dir(&d).unwrap();
        let f = d.join("endpoint.json");
        write_private(&f, b"one").unwrap();
        // A temp file left by a launcher that stopped mid-write doesn't stop the next one.
        std::fs::write(tmp_path(&f), b"stale").unwrap();
        write_private(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert!(is_private(&f).is_ok());
        let names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["endpoint.json".to_string()]);
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn an_ordinary_file_is_not_private() {
        let d = temp("plain");
        let f = d.join("plain.json");
        std::fs::write(&f, b"{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        // Windows: a new file takes the temp folder's inherited list, which isn't protected.
        assert!(is_private(&f).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn an_open_folder_of_this_users_is_closed_to_others() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("open").join("control-api");
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(is_private(&d).is_err());
        private_dir(&d).unwrap();
        assert!(is_private(&d).is_ok());
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_is_refused() {
        let d = temp("link");
        std::fs::create_dir_all(d.join("real")).unwrap();
        std::os::unix::fs::symlink(d.join("real"), d.join("control-api")).unwrap();
        assert!(private_dir(&d.join("control-api")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_anyone_can_change_above_it_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("shared");
        std::fs::create_dir(d.join("data")).unwrap();
        // Anyone could rename `data` away and put their own in its place.
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = private_dir(&d.join("data").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("other users could replace"), "{err}");
        assert!(!d.join("data").join("control-api").exists());
        // Sticky, like /tmp: only the owner of `data` may rename it, so it's fine.
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o1777)).unwrap();
        private_dir(&d.join("data").join("control-api")).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    fn chmod(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    fn link(target: &Path, at: &Path) {
        std::os::unix::fs::symlink(target, at).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_its_group_may_change_above_it_is_refused() {
        // Even when the group is this user's own: who else is in it, or has a named entry that
        // shows up as group write on Linux, isn't known.
        let d = temp("groupw");
        std::fs::create_dir(d.join("data")).unwrap();
        chmod(&d, 0o770);
        let err = private_dir(&d.join("data").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("its group can change it"), "{err}");
        assert!(!d.join("data").join("control-api").exists());
        // Sticky: only the owner of `data` (this user) may rename it.
        chmod(&d, 0o1770);
        private_dir(&d.join("data").join("control-api")).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The owner and mode rules alone, with owners this test can't make (another user, the
    /// sandbox's "nobody"). Arithmetic on the rule only: nothing here runs as a second user.
    #[cfg(unix)]
    #[test]
    fn only_this_users_and_roots_folders_and_links_are_trusted() {
        use super::imp::others_can_replace;
        let me = 1000;
        assert_eq!(others_can_replace(me, 0o700, true, me), None);
        assert_eq!(others_can_replace(0, 0o755, true, me), None);
        assert_eq!(others_can_replace(0, 0o1777, true, me), None);
        assert_eq!(others_can_replace(1001, 0o755, true, me), Some("it belongs to another user"));
        // Flatpak shows host folders it can't map as "nobody": not trusted as root any more.
        assert_eq!(others_can_replace(65534, 0o755, true, me), Some("it belongs to another user"));
        // This user's own group (gid == uid on Linux) is still a group.
        assert_eq!(others_can_replace(me, 0o770, true, me), Some("its group can change it"));
        assert_eq!(others_can_replace(me, 0o707, true, me), Some("anyone can change it"));
        // A link: its owner matters (it may rename it in a sticky folder), its mode doesn't.
        assert_eq!(others_can_replace(me, 0o777, false, me), None);
        assert_eq!(others_can_replace(1001, 0o777, false, me), Some("it belongs to another user"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_a_folder_others_may_change_is_refused_wherever_it_points() {
        // The review's case: trusted/data is safe, but `shared/link` to it could be pointed
        // elsewhere by anyone, later.
        let d = temp("sharedlink");
        std::fs::create_dir_all(d.join("trusted").join("data")).unwrap();
        std::fs::create_dir(d.join("shared")).unwrap();
        chmod(&d.join("shared"), 0o777);
        link(&d.join("trusted").join("data"), &d.join("shared").join("link"));
        let err = private_dir(&d.join("shared").join("link").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("anyone can change it"), "{err}");
        assert!(!d.join("trusted").join("data").join("control-api").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_whose_target_goes_through_a_folder_others_may_change_is_refused() {
        let d = temp("unsafetarget");
        std::fs::create_dir_all(d.join("shared").join("data")).unwrap();
        chmod(&d.join("shared"), 0o777);
        link(&d.join("shared").join("data"), &d.join("link"));
        let err = private_dir(&d.join("link").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("anyone can change it"), "{err}");
        // The same through a relative target.
        link(Path::new("shared/data"), &d.join("rel"));
        let err = private_dir(&d.join("rel").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("anyone can change it"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn links_whose_whole_route_is_safe_are_fine() {
        // Like /var -> private/var on macOS (the temp folder these tests use is under it): this
        // user's links in this user's folders, two deep, one relative.
        let d = temp("safelinks");
        std::fs::create_dir_all(d.join("real").join("data")).unwrap();
        link(Path::new("real/data"), &d.join("inner"));
        link(&d.join("inner"), &d.join("outer"));
        let dir = d.join("outer").join("control-api");
        private_dir(&dir).unwrap();
        write_private(&dir.join("endpoint.json"), b"x").unwrap();
        assert_eq!(std::fs::read(d.join("real").join("data").join("control-api").join("endpoint.json")).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_safe_link_to_a_link_in_a_shared_folder_is_refused() {
        let d = temp("nested");
        std::fs::create_dir_all(d.join("real").join("data")).unwrap();
        std::fs::create_dir(d.join("shared")).unwrap();
        chmod(&d.join("shared"), 0o777);
        link(&d.join("real").join("data"), &d.join("shared").join("hop"));
        link(&d.join("shared").join("hop"), &d.join("first"));
        let err = private_dir(&d.join("first").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("anyone can change it"), "{err}");
        assert!(!d.join("real").join("data").join("control-api").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_loop_of_links_is_refused() {
        let d = temp("loop");
        link(&d.join("b"), &d.join("a"));
        link(&d.join("a"), &d.join("b"));
        let err = private_dir(&d.join("a").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("too many links"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_folder_named_on_the_way_is_refused_even_when_dot_dot_leaves_it() {
        // shared/sub/../../trusted is trusted by name, but the system goes through `shared` (and
        // whatever `sub` is then) to get there.
        let d = temp("dotdot");
        std::fs::create_dir_all(d.join("trusted")).unwrap();
        std::fs::create_dir_all(d.join("shared").join("sub")).unwrap();
        chmod(&d.join("shared"), 0o777);
        let dir = d.join("shared").join("sub").join("..").join("..").join("trusted").join("control-api");
        let err = private_dir(&dir).unwrap_err();
        assert!(err.to_string().contains("anyone can change it"), "{err}");
        assert!(private_dir(&d.join("trusted").join("control-api")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `chmod +a` on macOS: an access list entry, as `ls -le` shows them.
    #[cfg(target_os = "macos")]
    fn add_entry(path: &Path, entry: &str) {
        let ok = std::process::Command::new("/bin/chmod").arg("+a").arg(entry).arg(path).status().unwrap().success();
        assert!(ok, "chmod +a {entry}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn entries_handed_down_by_the_folder_above_are_taken_off() {
        // The data folder hands an entry for everyone down to each new folder and file in it: what
        // let others read the token before, with the modes 0700 and 0600 all the same.
        let d = temp("inherited");
        let data = d.join("data");
        std::fs::create_dir(&data).unwrap();
        add_entry(&data, "everyone allow read,write,execute,list,search,add_file,add_subdirectory,file_inherit,directory_inherit");
        let dir = data.join("control-api");
        private_dir(&dir).unwrap();
        let f = dir.join("endpoint.json");
        write_private(&f, b"{\"token\":\"x\"}").unwrap();
        assert!(is_private(&dir).is_ok());
        assert!(is_private(&f).is_ok());
        let listed = std::process::Command::new("/bin/ls").arg("-lde").arg(&dir).arg(&f).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout);
        assert!(!listed.contains("everyone"), "{listed}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_already_open_to_everyone_is_closed() {
        // The review's fixture: the control folder itself lets everyone in, and hands that down.
        let d = temp("aclfolder");
        let dir = d.join("control-api");
        std::fs::create_dir(&dir).unwrap();
        add_entry(&dir, "everyone allow read,write,execute,delete,add_file,add_subdirectory,delete_child,file_inherit,directory_inherit");
        assert!(is_private(&dir).is_err());
        private_dir(&dir).unwrap();
        let f = dir.join("endpoint.json");
        write_private(&f, b"x").unwrap();
        assert!(is_private(&f).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_0600_file_with_an_entry_for_everyone_is_not_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("aclfile");
        let f = d.join("endpoint.json");
        std::fs::write(&f, b"x").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        add_entry(&f, "everyone allow read");
        assert!(is_private(&f).unwrap_err().to_string().contains("access list"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_link_on_the_way_whose_entry_lets_others_delete_it_is_refused() {
        // The link itself, not where it leads: whoever may delete it may put another in its place.
        let d = temp("acllink");
        std::fs::create_dir_all(d.join("real").join("data")).unwrap();
        link(&d.join("real").join("data"), &d.join("link"));
        let ok = std::process::Command::new("/bin/chmod").args(["-h", "+a", "everyone allow delete"]).arg(d.join("link")).status().unwrap().success();
        assert!(ok, "chmod -h +a");
        let err = private_dir(&d.join("link").join("control-api")).unwrap_err();
        assert!(err.to_string().contains("access list"), "{err}");
        assert!(!d.join("real").join("data").join("control-api").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_above_whose_entry_lets_others_rename_is_refused() {
        let d = temp("aclabove");
        std::fs::create_dir(d.join("data")).unwrap();
        add_entry(&d, "everyone allow delete_child");
        assert!(private_dir(&d.join("data").join("control-api")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    mod on_windows {
        use super::*;
        use ::windows::core::HSTRING;
        use ::windows::Win32::Foundation::{LocalFree, HLOCAL};
        use ::windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
        use ::windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        use ::windows::Win32::Storage::FileSystem::CreateDirectoryW;

        /// A folder made with this access list (SDDL). Err when Windows won't make it so (an owner
        /// other than this user needs administrator rights).
        fn dir_with(path: &Path, sddl: &str) -> ::windows::core::Result<()> {
            unsafe {
                let mut sd = PSECURITY_DESCRIPTOR::default();
                ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), SDDL_REVISION_1, &mut sd, None)?;
                let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
                let made = CreateDirectoryW(&HSTRING::from(path), Some(&sa));
                let _ = LocalFree(Some(HLOCAL(sd.0)));
                made
            }
        }

        fn my_sid() -> String {
            super::super::imp::me().unwrap()
        }

        #[test]
        fn a_folder_that_was_there_and_isnt_private_is_refused_not_repaired() {
            // Made the ordinary way, it takes the temp folder's list: someone allowed in then
            // might have it open still, so it isn't made private now.
            let d = temp("existing").join("control-api");
            std::fs::create_dir(&d).unwrap();
            assert!(private_dir(&d).is_err());
            assert!(is_private(&d).is_err());
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }

        #[test]
        fn a_folder_above_that_others_may_change_is_refused() {
            let d = temp("shared");
            let data = d.join("data");
            // Everyone may delete what's in it: they could replace control-api.
            dir_with(&data, &format!("D:P(A;OICI;FA;;;{})(A;OICI;0x40;;;WD)", my_sid())).unwrap();
            let err = private_dir(&data.join("control-api")).unwrap_err();
            assert!(err.to_string().contains("other users could replace"), "{err}");
            assert!(!data.join("control-api").exists());
            let _ = std::fs::remove_dir_all(&d);
        }

        #[test]
        fn a_folder_owned_by_someone_else_is_refused() {
            let d = temp("foreign").join("control-api");
            // Owned by Administrators, with only this user in its list: the owner could still
            // change the list. Windows only lets an administrator make one like it.
            if let Err(e) = dir_with(&d, &format!("O:BAD:P(A;OICI;FA;;;{})", my_sid())) {
                eprintln!("skipped: an owner other than this user needs administrator rights here ({e})");
                return;
            }
            let err = private_dir(&d).unwrap_err();
            assert!(err.to_string().contains("another user"), "{err}");
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }

        #[test]
        fn a_folder_whose_one_entry_isnt_this_user_is_refused() {
            let d = temp("wrongsid").join("control-api");
            // Protected, this user's, one entry: but the entry is for everyone.
            dir_with(&d, &format!("O:{}D:P(A;OICI;FA;;;WD)", my_sid())).unwrap();
            assert!(private_dir(&d).is_err());
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }
}
