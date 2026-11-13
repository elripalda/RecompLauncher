use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::{BufRead, BufReader, Write}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::mpsc, time::{Duration, Instant}};

#[derive(Debug)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}
impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, cwd: &Path) -> Self { Self { program:program.into(), args:vec![], env:BTreeMap::new(), cwd:cwd.into() } }
}

#[derive(Serialize, Deserialize)]
pub struct Journal {
    pub state: String,
    pub installation: String,
    pub package: String,
    pub inputs: String,
}
impl Journal {
    pub fn save(&self, root: &Path) -> Result<(), String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let temp = root.join("job.json.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(self).unwrap()).map_err(|e| e.to_string())?;
        fs::rename(temp, root.join("job.json")).map_err(|e| e.to_string())
    }
}

fn stop_group(child: &mut std::process::Child) {
    #[cfg(unix)] {
        unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
        let pid = child.id() as i32;
        unsafe { kill(-pid, 15); }
        std::thread::sleep(Duration::from_millis(500));
        unsafe { kill(-pid, 9); }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Own a process group so cancelling preparation cannot leave compilers running.
pub fn run_process(spec: &CommandSpec, log: &Path, cancelled: &dyn Fn() -> bool, event: &dyn Fn(serde_json::Value)) -> Result<(), String> {
    if cancelled() { return Err("Stopped.".into()); }
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).envs(&spec.env).current_dir(&spec.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // The packaged interpreter/toolchain must not inherit a developer's overrides.
    for name in ["PYTHONPATH", "PYTHONHOME", "DM_GAMES_DIR", "DM_GAMES_FIRST", "DM_ALLOW_OTHER_METAL", "DM_EXPERIMENTAL_BUILDER", "NXB_BIN", "SPIRV_CROSS"] { cmd.env_remove(name); }
    #[cfg(unix)] { use std::os::unix::process::CommandExt; cmd.process_group(0); }
    #[cfg(windows)] { use std::os::windows::process::CommandExt; use windows::Win32::System::Threading::{CREATE_SUSPENDED, CREATE_NO_WINDOW}; cmd.creation_flags(CREATE_SUSPENDED.0 | CREATE_NO_WINDOW.0); }
    let mut log = fs::File::create(log).map_err(|e| e.to_string())?;
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start game preparation: {e}"))?;
    #[cfg(windows)]
    let job = match super::windows_job::Job::attach(&child) {
        Ok(job) => job,
        Err(e) => { stop_group(&mut child); return Err(e); }
    };
    let (tx, rx) = mpsc::channel();
    fn reader(input: impl std::io::Read + Send + 'static, tx: mpsc::Sender<String>) {
        std::thread::spawn(move || {
            for line in BufReader::new(input).lines().map_while(Result::ok) {
                if tx.send(line).is_err() { break; }
            }
        });
    }
    reader(child.stdout.take().unwrap(), tx.clone());
    reader(child.stderr.take().unwrap(), tx.clone());
    drop(tx);
    let start = Instant::now();
    let mut status = None;
    let mut exited = None;
    let mut last_error = String::new();
    loop {
        if cancelled() || start.elapsed() > Duration::from_secs(6 * 3600) {
            #[cfg(windows)] job.stop();
            stop_group(&mut child);
            return Err(if cancelled() { "Stopped." } else { "Preparation timed out. Its reusable work has been kept." }.into());
        }
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(line) => {
                let _ = writeln!(log, "{line}");
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(msg) = v["msg"].as_str().filter(|m| m.starts_with("[RED]")) { last_error = msg.chars().take(1000).collect(); }
                    event(v);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) if status.is_some() => break,
            _ => {},
        }
        if status.is_none() {
            match child.try_wait() { Ok(Some(s)) => { status = Some(s); exited = Some(Instant::now()); }, Ok(None) => {}, Err(e) => { stop_group(&mut child); return Err(e.to_string()); } }
        }
        if exited.is_some_and(|t| t.elapsed() > Duration::from_secs(1)) { stop_group(&mut child); break; }
    }
    let code = status.ok_or("Preparation ended without an exit status.")?;
    if !code.success() { return Err(format!("Game preparation failed ({code}). {last_error} See the preparation log in the game folder.")); }
    Ok(())
}
