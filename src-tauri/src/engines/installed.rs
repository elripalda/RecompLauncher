use super::{Components, GameIdentity, PackagePin, Target};
use crate::{library::Installed, profile::Profile};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Format 1 launches a prepared native output with its runtime embedded in the game folder.
/// Shared runtime packages will need an explicit resolver, not a global "latest" directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildPin {
    pub format: u32,
    pub game: GameIdentity,
    pub package: PackagePin,
    pub runtime: PackagePin,
    pub components: Components,
    pub profile: Profile,
    pub required_files: Vec<PathBuf>,
}

impl BuildPin {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err("This prepared game's record needs a different launcher version. Its files have been kept.".into());
        }
        self.game.validate()?;
        self.package.validate()?;
        self.runtime.validate()?;
        self.components.validate()?;
        if self.package.target != self.runtime.target {
            return Err("The recorded game and runtime are for different computers.".into());
        }
        let p = &self.profile;
        let start = p.start.as_ref().ok_or("The prepared game has no recorded launch command.")?;
        if p.format != 1 || p.game.is_some() || !p.inputs.is_empty() || p.prepare.is_some() || !p.actions.is_empty() || p.games.is_some() {
            return Err("A prepared game's launch profile must not run game preparation.".into());
        }
        if start.program.len() != 1 || !relative(Path::new(&start.program[0])) || self.required_files.iter().any(|p| !relative(p)) {
            return Err("The prepared game must name its exact program and runtime files inside its own folder.".into());
        }
        Ok(())
    }
}

pub fn read_build(g: &Installed) -> Result<Option<BuildPin>, String> {
    let Some(raw) = &g.engine_build else { return Ok(None) };
    let build: BuildPin = serde_json::from_value(raw.clone())
        .map_err(|_| "The saved engine build cannot be read by this launcher. Its files have been kept.")?;
    build.validate()?;
    Ok(Some(build))
}

pub fn pinned_profile(g: &Installed) -> Result<Option<Profile>, String> {
    Ok(read_build(g)?.map(|b| b.profile))
}

pub fn pinned_executable(g: &Installed, target: Target) -> Result<Option<PathBuf>, String> {
    let Some(build) = read_build(g)? else { return Ok(None) };
    if build.package.target != target {
        return Err("This prepared game is for a different computer. Its working version has been kept.".into());
    }
    let root = g.path.canonicalize().map_err(|_| "The prepared game's folder is missing.")?;
    for file in &build.required_files {
        contained_file(&root, file)?;
    }
    let exe = Path::new(&build.profile.start.as_ref().unwrap().program[0]);
    let exe = contained_file(&root, exe)?;
    let native = native_binary(&exe, target);
    if !native {
        return Err("The recorded program is not a prepared native game for this computer. Setup scripts cannot run from Play.".into());
    }
    Ok(Some(exe))
}

pub(super) fn native_binary(exe: &Path, target: Target) -> bool {
    match target {
        Target::MacosArm64 => crate::macos::is_executable(exe, "arm64"),
        Target::LinuxX64 => crate::proton::elf_arch(exe).as_deref() == Some("x64"),
        Target::WindowsX64 => windows_x64(exe),
    }
}

pub fn executable_for_host(g: &Installed) -> Result<Option<PathBuf>, String> {
    if g.engine_build.is_none() {
        return Ok(None);
    }
    let target = Target::current().ok_or("Prepared engine games are not supported on this computer.")?;
    pinned_executable(g, target)
}

fn windows_x64(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let check = || -> Option<bool> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut dos = [0u8; 64];
        file.read_exact(&mut dos).ok()?;
        if &dos[..2] != b"MZ" {
            return Some(false);
        }
        let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?);
        file.seek(SeekFrom::Start(offset.into())).ok()?;
        let mut pe = [0u8; 6];
        file.read_exact(&mut pe).ok()?;
        Some(&pe[..4] == b"PE\0\0" && pe[4..] == [0x64, 0x86])
    };
    check().unwrap_or(false)
}

fn contained_file(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let file = root.join(relative).canonicalize().map_err(|_| {
        format!("A file required by this prepared game is missing: {}. Restore this version's files before playing.", relative.display())
    })?;
    if !file.starts_with(root) || !file.is_file() {
        return Err("The prepared game's program or runtime file is outside its recorded folder.".into());
    }
    Ok(file)
}

fn relative(path: &Path) -> bool {
    let s = path.to_string_lossy();
    !s.is_empty() && !s.contains(['\\', ':', '{', '}']) && path.components().all(|c| matches!(c, Component::Normal(_)))
}
