use recomplauncher_lib::engines::nxnative::ProgressTracker;
use serde_json::json;

#[test]
fn current_step_counts_and_eta_survive_the_adapter() {
    let mut progress = ProgressTracker::default();
    progress.event(&json!({"ev":"stage","k":5,"n":11,"name":"Mac shaders","step":""}));
    let p = progress.event(&json!({"ev":"progress","k":5,"name":"Mac shaders","step":"compiling","done":951,"total":1812,"unit":"shaders","est":false,"note":"1,812 not cached","frac":0.5248344370860927,"eta":5.465026058397795,"elapsed":9.889})).unwrap();
    assert_eq!(p.text,"Mac shaders · compiling");
    assert_eq!((p.stage,p.stages),(Some(5),Some(11)));
    assert!((p.fraction.unwrap()-0.5248344).abs()<0.00001);
    assert_eq!((p.done,p.total),(Some(951.0),Some(1812.0)));
    assert_eq!(p.unit,"shaders");
    assert_eq!(p.eta_seconds,Some(5.465026058397795));
    assert_eq!(p.elapsed_seconds,Some(9.889));
}

#[test]
fn new_steps_clear_counts_and_eta_and_empty_steps_keep_the_stage_name() {
    let mut progress = ProgressTracker::default();
    let stage=progress.event(&json!({"ev":"stage","k":2,"n":8,"name":"extract","step":""})).unwrap();
    assert_eq!(stage.text,"extract");
    progress.event(&json!({"ev":"progress","k":2,"name":"extract","step":"base","frac":0.9,"eta":4,"done":90,"total":100}));
    let next=progress.event(&json!({"ev":"step","k":2,"name":"extract","step":"update"})).unwrap();
    assert_eq!(next.stages,Some(8));
    assert!(next.fraction.is_none() && next.eta_seconds.is_none() && next.done.is_none());
}

#[test]
fn only_a_successful_stage_end_completes_its_bar() {
    let mut progress = ProgressTracker::default();
    let zero=progress.event(&json!({"ev":"progress","k":1,"name":"test","frac":0,"eta":0,"elapsed":0,"est":true})).unwrap();
    assert_eq!(zero.fraction,Some(0.0)); assert_eq!(zero.eta_seconds,Some(0.0)); assert!(zero.estimated);
    let bad=progress.event(&json!({"ev":"progress","k":1,"name":"test","frac":-1,"eta":-5,"total":0})).unwrap();
    assert!(bad.fraction.is_none() && bad.eta_seconds.is_none() && bad.total.is_none());
    assert!(progress.event(&json!({"ev":"stage_end","k":1,"name":"test","ok":false})).unwrap().fraction.is_none());
    assert_eq!(progress.event(&json!({"ev":"stage_end","k":1,"name":"test","ok":true})).unwrap().fraction,Some(1.0));
    assert!(progress.event(&json!({"ev":"say","msg":"log message"})).is_none());
}

#[test]
#[ignore = "set RL_ENGINE_PROGRESS_LOG to a local nxnative JSON preparation log"]
fn recorded_build_progress_replays_without_preparing_the_game_again() {
    let log = std::fs::read_to_string(std::env::var("RL_ENGINE_PROGRESS_LOG").unwrap()).unwrap();
    let mut tracker = ProgressTracker::default();
    let (mut measurements, mut estimates, mut completions) = (0,0,0);
    for line in log.lines() {
        let Ok(event) = serde_json::from_str(&line) else { continue };
        let Some(p) = tracker.event(&event) else { continue };
        if let Some(f) = p.fraction { assert!((0.0..=1.0).contains(&f)); measurements += 1; }
        if p.eta_seconds.is_some_and(|eta| eta > 0.0) { estimates += 1; }
        if event["ev"] == "stage_end" && event["ok"] == true { assert_eq!(p.fraction,Some(1.0)); completions += 1; }
        if event["ev"] == "step" { assert!(p.eta_seconds.is_none()); }
    }
    assert!(measurements > 0 && estimates > 0 && completions > 1);
    println!("Replayed {measurements} measured updates, {estimates} ETAs and {completions} completed stages.");
}
