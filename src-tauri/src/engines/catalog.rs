use super::{registry, Target};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineGame {
    #[serde(default = "legacy_engine", skip_serializing_if = "is_legacy_engine")]
    pub engine: String,
    pub title_id: String,
    pub name: String,
    pub console: String,
    pub package: String,
    pub version: String,
    pub build_id: String,
    pub notes: String,
    pub evidence: String,
}

// The first saved format predates the registry. This is record compatibility, not dispatch.
fn legacy_engine() -> String { "nxnative".into() }
fn is_legacy_engine(engine: &String) -> bool { engine == "nxnative" }

impl EngineGame {
    pub fn id(&self) -> String { format!("{}-{}", self.engine, self.title_id.to_lowercase()) }
    pub fn setup_profile(&self) -> Profile { registry::get(&self.engine).expect("validated engine record").profile(self) }
}

pub fn game(id: &str) -> Option<EngineGame> {
    super::ADAPTERS.iter().flat_map(|e| e.games()).find(|g| g.id() == id)
}
pub fn is_engine(id: &str) -> bool { super::ADAPTERS.iter().any(|e| id.starts_with(&format!("{}-", e.id()))) }

pub fn merge(cat: &mut Value, target: Option<Target>) {
    let Some(target) = target else { return };
    if !cat.is_object() { *cat = json!({"apps":[]}); }
    if !cat["apps"].is_array() { cat["apps"] = json!([]); }
    let apps = cat["apps"].as_array_mut().unwrap();
    for engine in registry::available(target) {
        let pin = engine.pin(target).expect("catalog adapter has a pinned package");
        let systems: Vec<_> = engine.targets().iter().map(|t| match t { Target::MacosArm64 => "macos", Target::WindowsX64 => "windows", Target::LinuxX64 => "linux" }).collect();
        for g in engine.games() {
            if apps.iter().any(|a| a["id"].as_str() == Some(&g.id())) { continue; }
            let title = apps.iter().find(|a| a["game"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&g.name)))
                .and_then(|a| a["title"].as_str()).map(str::to_owned).unwrap_or_else(|| format!("{}-{}", g.console, g.title_id.to_lowercase()));
            let artwork = engine.artwork(&g.title_id);
            apps.push(json!({"id":g.id(),"name":g.name,"game":g.name,"title":title,"origin":g.console,
                "cover":artwork["cover"],"hero":artwork["hero"],"repo":engine.repo(),"systems":systems,
                "type":"recomp","engine":engine.id(),"adds":format!("{} · {}",engine.id(),engine.status()),
                "engine_version":super::package_label(&pin),"engine_evidence":format!("https://github.com/{}/tree/{}",engine.repo(),pin.upstream_revision),
                "metadata_source":g.evidence,"experimental":true,"description":g.notes,
                "launcher":engine.profile(&g),"boot":{"files":{"how":"inputs","what":"Choose your game files."}}}));
        }
    }
}
