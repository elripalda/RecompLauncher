//! Where the launcher's setup flow meets the engines. Kept out of lib.rs because that file changes
//! constantly: lib.rs calls these with one line each, so its changes rarely collide with engine work.

use super::*;

/// Holds a game's place in `App::moving` while its files or record change, so a move, an uninstall
/// and an engine setup can't overlap. Released when dropped, including on an early error.
pub(crate) struct InstallationMutation { moving: Arc<Mutex<HashSet<String>>>, id: String }

impl InstallationMutation {
    pub(crate) fn acquire(st: &App, id: &str) -> Res<Self> {
        if !st.moving.lock().unwrap().insert(id.into()) { return Err(Error::Msg(MOVING.into())); }
        Ok(Self { moving: st.moving.clone(), id: id.into() })
    }
}

impl Drop for InstallationMutation {
    fn drop(&mut self) { self.moving.lock().unwrap().remove(&self.id); }
}

/// An engine game's setup answer. A new release never makes it "rebuild": only a validated native
/// output counts, and a prepared game keeps its pin until the player chooses to apply a fix.
pub(crate) fn status(g: &Installed) -> Option<setup_state::Status> {
    g.engine_build.as_ref()?;
    let ready = engines::executable_for_host(g).is_ok_and(|exe| exe.is_some());
    let reason = if ready { "ready" } else { "files" };
    Some(setup_state::Status { ready, reason, detail: String::new(), changed: false })
}

/// Install pressed on an engine game: nothing downloads yet. It's recorded as unfinished, and Setup
/// provisions the engine once the player has picked their files.
pub(crate) fn begin_install(app: &AppHandle, st: &App, id: &str, game: &engines::catalog::EngineGame, dir: Option<PathBuf>) -> Res<()> {
    let target = engines::Target::current().ok_or_else(|| Error::Msg("No game engine is available for this computer.".into()))?;
    engines::preflight(&game.engine, target).map_err(Error::Msg)?;
    let root = dir.unwrap_or_else(|| st.lib.lock().unwrap().settings.library_dir.clone());
    let g = engines::pending::create_for(game, target, &root.join(folder_name(id))).map_err(Error::Msg)?;
    {
        let mut l = st.lib.lock().unwrap();
        let old = l.clone();
        l.put(id, g.clone());
        l.settings.asked_location = true;
        if let Err(e) = l.save(&st.dir) { *l = old; let _ = std::fs::remove_dir(&g.path); return Err(e.into()); }
    }
    st.jobs.lock().unwrap().remove(id);
    let _ = app.emit("install", Progress::new(id, "done", 1, 1));
    Ok(())
}

/// A completed result is recovered from the files it was built with, so they can't change first.
pub(crate) fn check_pick(g: &Installed) -> Result<(), String> {
    if !engines::pending::awaiting_registration(g) { return Ok(()); }
    Err("Finish Setup to register the completed build before changing its selected files.".into())
}

/// How a prepared game starts: its recorded program, with nothing worked out from its file type.
pub(crate) fn pinned_command(exe: &Path) -> (PathBuf, String, tokio::process::Command) {
    (exe.to_path_buf(), String::new(), tokio::process::Command::new(exe))
}

/// Setup pressed on an unfinished engine game. The window's picks wait in memory for ordinary games;
/// an engine result is recovered from the record, so they're saved before preparation starts.
pub(crate) fn start(app: AppHandle, st: &App, id: String, mut g: Installed, copy: Option<String>) -> Res<()> {
    check_pick(&g).map_err(Error::Msg)?;
    let p = engines::pending::profile(&g).map_err(Error::Msg)?.ok_or_else(|| Error::Msg("No pending game preparation.".into()))?;
    let main = p.main_input().cloned();
    if let (Some(m), Some(path)) = (main.as_ref(), copy.filter(|c| !c.is_empty())) {
        let taken = take_pick(st, &p, m, &g, PathBuf::from(path)).map_err(Error::Msg)?;
        g.setup_inputs.insert(m.id.clone(), taken);
    }
    let saved = save_picks(st, &id, &g)?;
    clear_pending(&id);
    engine_setup(app, st, id, saved)
}

pub(crate) fn save_picks(st: &App, id: &str, picked: &Installed) -> Res<Installed> {
    let mut library = st.lib.lock().unwrap();
    let current = library.installed.get(id).cloned().ok_or_else(|| Error::Msg(MOVING.into()))?;
    if !engines::pending::same_installation(&current, picked) {
        return Err(Error::Msg("The installation changed while selecting files. Open Easy Setup again.".into()));
    }
    if current.setup_inputs == picked.setup_inputs { return Ok(current); }
    let mut next = current;
    next.setup_inputs = picked.setup_inputs.clone();
    next.inputs_ready.clear();
    let old = library.clone();
    library.put(id, next.clone());
    if let Err(error) = library.save(&st.dir) { *library = old; return Err(error.into()); }
    Ok(next)
}

pub(crate) fn engine_setup_matches(expected: &Installed, current: &Installed) -> bool {
    engines::pending::same_installation(expected, current)
        && expected.setup_inputs == current.setup_inputs && expected.runtime_off == current.runtime_off
        && expected.input_staged == current.input_staged && expected.prepare_opts == current.prepare_opts
}

pub(crate) fn refresh_engine_setup(st: &App, run: u64, original: &Installed, cancel: &AtomicBool) -> Result<(Installed, String), String> {
    let stopped = || cancel.load(Ordering::Relaxed) || setup_stopped(&original.id, run);
    if stopped() { return Err("Stopped.".into()); }
    let snapshot = {
        let library = st.lib.lock().unwrap();
        let current = library.installed.get(&original.id).ok_or(MOVING)?;
        if !engine_setup_matches(original, current) { return Err("The installation or selected files changed. Retry Setup.".into()); }
        current.clone()
    };
    // Checkpoint checks and recursive input fingerprints can touch large folders.
    let refreshed = engines::pending::for_setup(&snapshot)?;
    let profile = engines::pending::profile(&refreshed)?.ok_or("No pending game preparation.")?;
    let signature = engines::input_signature_with_cancel(&profile, &refreshed, &st.dir, &stopped)?;
    let runs = SETUP_RUN.lock().unwrap();
    if runs.as_ref().and_then(|m| m.get(&original.id)) != Some(&run) || cancel.load(Ordering::Relaxed) { return Err("Stopped.".into()); }
    let mut library = st.lib.lock().unwrap();
    let current = library.installed.get(&original.id).ok_or(MOVING)?;
    if !engine_setup_matches(&snapshot, current) { return Err("The installation or selected files changed. Retry Setup.".into()); }
    let mut next = current.clone();
    next.engine_build = refreshed.engine_build;
    next.tag = refreshed.tag;
    next.inputs_ready = refreshed.inputs_ready;
    if snapshot.engine_build != next.engine_build {
        let old = library.clone();
        library.put(&original.id, next.clone());
        if let Err(error) = library.save(&st.dir) { *library = old; return Err(error.to_string()); }
    }
    Ok((next, signature))
}

pub(crate) fn apply_engine_result(current: &mut Installed, result: &Installed) {
    // Activation may finish after preferences change. Publish only the prepared build's fields.
    current.engine_build = result.engine_build.clone();
    current.exe = result.exe.clone();
    current.version = result.version.clone();
    current.tag = result.tag.clone();
    current.asset = result.asset.clone();
    current.sha256 = result.sha256.clone();
    current.verified = result.verified;
    current.inputs_ready = result.inputs_ready.clone();
    current.files = result.files.clone();
    current.auto_update = result.auto_update;
    current.pinned = result.pinned;
    current.mode = result.mode;
}

pub(crate) fn finish_engine_setup(
    st: &App, run: u64, expected: &Installed, signature: &str, ready: &Installed, cancel: &AtomicBool,
    activate: &dyn Fn(&Installed, &Installed, &dyn Fn() -> bool) -> Result<Installed, String>,
) -> Result<(), String> {
    let stopped = || cancel.load(Ordering::Relaxed) || setup_stopped(&expected.id, run);
    if stopped() { return Err("Stopped.".into()); }
    let snapshot = {
        let library = st.lib.lock().unwrap();
        let current = library.installed.get(&expected.id).ok_or("The game was removed during preparation.")?;
        if !engine_setup_matches(expected, current) { return Err("The installation changed during preparation. The result has been kept.".into()); }
        current.clone()
    };
    let profile = engines::pending::profile(expected)?.ok_or("No pending game preparation.")?;
    if ready.inputs_ready != signature || engines::input_signature_with_cancel(&profile, &snapshot, &st.dir, &stopped)? != signature {
        return Err("The selected files changed during preparation. The result has been kept.".into());
    }
    let active = activate(&snapshot, ready, &stopped)?;
    let runs = SETUP_RUN.lock().unwrap();
    if runs.as_ref().and_then(|m| m.get(&expected.id)) != Some(&run) || cancel.load(Ordering::Relaxed) { return Err("Stopped.".into()); }
    let mut library = st.lib.lock().unwrap();
    let current = library.installed.get(&expected.id).ok_or("The game was removed during preparation.")?;
    if !engine_setup_matches(&snapshot, current) { return Err("The installation changed during preparation. The result has been kept.".into()); }
    let mut merged = current.clone();
    apply_engine_result(&mut merged, &active);
    let old = library.clone();
    library.put(&expected.id, merged);
    if let Err(error) = library.save(&st.dir) { *library = old; return Err(error.to_string()); }
    Ok(())
}

pub(crate) fn engine_setup(app: AppHandle, st: &App, id: String, g: Installed) -> Res<()> {
    let mutation = InstallationMutation::acquire(st, &id)?;
    INPUT_WORKERS.lock().unwrap().push(id.clone());
    let run = SETUP_NEXT.fetch_add(1, Ordering::Relaxed);
    SETUP_RUN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), run);
    let data = st.dir.clone();
    let http = st.http.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let (watch_id, watch_cancel, watch_finished) = (id.clone(), cancel.clone(), finished.clone());
    std::thread::spawn(move || {
        while !watch_finished.load(Ordering::Relaxed) {
            if setup_stopped(&watch_id, run) { watch_cancel.store(true, Ordering::Relaxed); break; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    std::thread::spawn(move || {
        let _mutation = mutation;
        let emit = |ev: SetupEv| {
            let ev = SetupEv { id: id.clone(), extras: true, ..ev };
            setup_track(&ev);
            let _ = app.emit("setup", ev);
        };
        emit(SetupEv { step: "read".into(), text: "Checking selected game files".into(), ..Default::default() });
        let st = app.state::<App>();
        let result = (|| {
            let (g, signature) = refresh_engine_setup(&st, run, &g, &cancel)?;
            let ready = engines::coordinator::prepare(&http, &data, &g, cancel.clone(), &|progress| {
                emit(SetupEv { step: "extras".into(), pct: progress.fraction.unwrap_or(0.0), text: progress.text.clone(), engine_progress: Some(progress), ..Default::default() });
            })?;
            finish_engine_setup(&st, run, &g, &signature, &ready, &cancel, &engines::coordinator::activate_output)
        })();
        finished.store(true, Ordering::Relaxed);
        if !setup_stopped(&id, run) {
            match result {
                Ok(()) => emit(SetupEv { step: "done".into(), pct: 1.0, text: "Your game is ready to play.".into(), ..Default::default() }),
                Err(e) => emit(SetupEv { step: "error".into(), text: e, ..Default::default() }),
            }
        }
        if let Some(m) = SETUP_RUN.lock().unwrap().as_mut() { if m.get(&id) == Some(&run) { m.remove(&id); } }
        if let Some(m) = SETUP_STATUS.lock().unwrap().as_mut() { m.remove(&id); }
        INPUT_WORKERS.lock().unwrap().retain(|x| x != &id);
    });
    Ok(())
}
