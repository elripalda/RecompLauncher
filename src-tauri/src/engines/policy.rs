use super::{model::fingerprint, read_build, Components, GameIdentity, PackagePin, Target, Work};
use crate::library::Installed;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub game: GameIdentity,
    pub package: PackagePin,
    /// Lower values are preferred. Stable IDs break ties without another user decision.
    pub rank: u32,
    pub tested: bool,
    pub evidence: String,
}

pub fn select_route<'a>(routes: &'a [Route], game: &GameIdentity, target: Target, selected: Option<&str>) -> Result<&'a Route, String> {
    game.validate()?;
    let eligible = |r: &&Route| {
        r.tested
            && !r.evidence.trim().is_empty()
            && !r.id.is_empty()
            && r.game.matches(game)
            && r.package.target == target
            && r.package.validate().is_ok()
    };
    let choices: Vec<_> = routes.iter().filter(eligible).collect();
    let mut ids = std::collections::BTreeSet::new();
    if choices.iter().any(|r| !ids.insert(&r.id)) {
        return Err("The available versions contain conflicting entries. Keep the installed version and refresh the list later.".into());
    }
    let mut choices = choices.into_iter();
    if let Some(id) = selected {
        return choices.find(|r| r.id == id).ok_or_else(|| "The selected version cannot prepare this copy on this computer.".into());
    }
    choices
        .min_by(|a, b| (a.rank, &a.id).cmp(&(b.rank, &b.id)))
        .ok_or_else(|| "No tested installation is available for this game version on this computer.".into())
}

pub fn release_updates_allowed(g: &Installed) -> bool {
    !g.local && g.engine_build.is_none()
}

pub fn ensure_legacy_install(g: Option<&Installed>) -> Result<(), String> {
    if g.is_some_and(|g| g.engine_build.is_some()) {
        return Err("This prepared game keeps its working version. Apply a relevant game fix to replace it.".into());
    }
    Ok(())
}

pub fn ensure_legacy_setup(g: &Installed) -> Result<(), String> {
    ensure_legacy_install(Some(g))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    pub id: String,
    pub game: GameIdentity,
    pub from: PackagePin,
    pub to: PackagePin,
    pub components: Components,
    pub evidence: String,
    pub summary: String,
}

impl Fix {
    fn work_for(&self, g: &Installed) -> Result<Work, String> {
        let current = read_build(g)?.ok_or("This installation has no engine build.")?;
        self.game.validate()?;
        self.from.validate()?;
        self.to.validate()?;
        self.components.validate()?;
        let relevant = self.game.matches(&current.game) && self.from == current.package;
        let same_route = self.to.engine == current.package.engine && self.to.target == current.package.target;
        if !relevant || !same_route || self.from == self.to {
            return Err("This fix does not apply to the installed game and its working version.".into());
        }
        if self.id.trim().is_empty() || self.summary.trim().is_empty() || !self.evidence.starts_with("https://") {
            return Err("This fix needs a description and evidence of its relevance.".into());
        }
        let work = current.components.work_for(&self.components);
        if work == Work::default() {
            return Err("This engine update does not change this game's prepared components.".into());
        }
        Ok(work)
    }
}

/// Created only by an explicit Apply Fix action; never by discovery, Play, or an update check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixConsent {
    installation_id: String,
    build_fingerprint: String,
    fix_fingerprint: String,
}

impl FixConsent {
    pub fn for_fix(g: &Installed, fix: &Fix) -> Result<Self, String> {
        fix.work_for(g)?;
        if g.id.is_empty() {
            return Err("The fix must belong to a saved installation.".into());
        }
        Ok(Self { installation_id: g.id.clone(), build_fingerprint: installation_fingerprint(g), fix_fingerprint: fingerprint(fix) })
    }

    pub fn authorize(&self, g: &Installed, fix: &Fix) -> Result<Work, String> {
        if self.installation_id != g.id || self.build_fingerprint != installation_fingerprint(g) || self.fix_fingerprint != fingerprint(fix)
        {
            return Err("The installation or fix has changed. Choose the fix again before preparing a replacement.".into());
        }
        fix.work_for(g)
    }
}

fn installation_fingerprint(g: &Installed) -> String {
    fingerprint(&(&g.engine_build, &g.path, g.installed_at))
}
