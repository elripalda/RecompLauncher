// Decorative drive activity while setup runs: rapid flickers mixed with longer reads.
// The pattern does not measure physical disk I/O or restart on each progress update.
function createSetupDriveLight() {
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  let light = null, timer = null, lit = false, flashesLeft = 0, rapid = false;
  const between = (min, max) => min + Math.random() * (max - min);
  function clearTimer() { if (timer !== null) clearTimeout(timer); timer = null; }
  function stop() {
    clearTimer();
    light?.classList.remove('on');
    light = null; lit = false; flashesLeft = 0;
    document.removeEventListener('visibilitychange', resume);
    reduced.removeEventListener('change', resume);
  }
  function tick() {
    clearTimer();
    if (!light?.isConnected) return stop();
    if (document.hidden) { light.classList.remove('on'); lit = false; return; }
    if (reduced.matches) { light.classList.add('on'); return; }
    let delay;
    if (!lit) {
      if (!flashesLeft) {
        rapid = Math.random() < .45;
        flashesLeft = rapid ? 2 + Math.floor(Math.random() * 4) : 1 + Math.floor(Math.random() * 4);
      }
      lit = true;
      delay = rapid ? between(45, 95) : between(650, 1600);
    } else {
      lit = false;
      flashesLeft--;
      delay = flashesLeft ? (rapid ? between(40, 100) : between(90, 230)) : between(150, 450);
    }
    light.classList.toggle('on', lit);
    timer = setTimeout(tick, delay);
  }
  function resume() { clearTimer(); lit = false; flashesLeft = 0; tick(); }
  function start(element) {
    if (light === element) return;
    stop();
    if (!element) return;
    light = element;
    document.addEventListener('visibilitychange', resume);
    reduced.addEventListener('change', resume);
    tick();
  }
  return { start, stop };
}
