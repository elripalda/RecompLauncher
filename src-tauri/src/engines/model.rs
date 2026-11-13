use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    #[serde(rename = "macos-arm64")]
    MacosArm64,
    #[serde(rename = "windows-x64")]
    WindowsX64,
    #[serde(rename = "linux-x64")]
    LinuxX64,
}

impl Target {
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Some(Self::MacosArm64),
            ("windows", "x86_64") => Some(Self::WindowsX64),
            ("linux", "x86_64") => Some(Self::LinuxX64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Console {
    Switch,
    Switch2,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameIdentity {
    pub console: Console,
    pub title_id: String,
    pub version: String,
    pub build_id: String,
}

impl GameIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if self.title_id.trim().is_empty() || self.version.trim().is_empty() || self.build_id.is_empty() {
            return Err("The game's title, version, and build must be identified before preparation.".into());
        }
        Ok(())
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.console == other.console
            && self.title_id.eq_ignore_ascii_case(&other.title_id)
            && self.version == other.version
            && self.build_id.eq_ignore_ascii_case(&other.build_id)
    }
}

/// Everything that distinguishes a distributed engine package, including a patched rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackagePin {
    pub engine: String,
    pub target: Target,
    pub upstream_revision: String,
    pub patch_sha256: String,
    pub toolchain_sha256: String,
    pub archive_sha256: String,
    pub packaging_revision: String,
    pub adapter_version: u32,
}

impl PackagePin {
    pub fn validate(&self) -> Result<(), String> {
        let named = !self.engine.is_empty() && self.engine.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-');
        let commit = hex_size(&self.upstream_revision, 40) || hex_size(&self.upstream_revision, 64);
        let digests = [&self.patch_sha256, &self.toolchain_sha256, &self.archive_sha256];
        if !named || !commit || digests.iter().any(|s| !hex_size(s, 64)) || self.packaging_revision.trim().is_empty() {
            return Err("The engine package does not identify an exact checkpoint and its contents.".into());
        }
        if self.adapter_version != 1 {
            return Err("This engine package needs a different launcher adapter.".into());
        }
        Ok(())
    }

    pub fn identity(&self) -> String {
        fingerprint(self)
    }
}

/// These describe the inputs that affect each output, not the upstream repository as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Components {
    pub code: String,
    pub shaders: String,
    pub runtime: String,
    pub data: String,
}

impl Components {
    pub fn validate(&self) -> Result<(), String> {
        if [&self.code, &self.shaders, &self.runtime, &self.data].iter().any(|s| !hex_size(s, 64)) {
            return Err("This build is missing its preparation fingerprints.".into());
        }
        Ok(())
    }

    /// An adapter must still support partial replacement before a job can use this smaller plan.
    pub fn work_for(&self, next: &Self) -> Work {
        Work {
            code: self.code != next.code,
            shaders: self.shaders != next.shaders,
            runtime: self.runtime != next.runtime,
            data: self.data != next.data,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub code: bool,
    pub shaders: bool,
    pub runtime: bool,
    pub data: bool,
}

pub(crate) fn fingerprint(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("engine records contain JSON values");
    hex::encode(Sha256::digest(bytes))
}

fn hex_size(s: &str, size: usize) -> bool {
    s.len() == size && s.bytes().all(|c| c.is_ascii_hexdigit())
}
