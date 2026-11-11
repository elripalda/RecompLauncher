# Big Picture performance investigation

The local Arch state had 601 detected game files and 103 saved library entries. The session
was GNOME on Wayland, with an NVIDIA RTX 5080 and Intel UHD 770 present. The running launcher's
WebKit child had `WEBKIT_DISABLE_DMABUF_RENDERER=1` and
`WEBKIT_DISABLE_COMPOSITING_MODE=1`. These are separate factors: the number of detected files
is not the number of tiles rendered, and the GPU present does not prove which GPU draws the UI.

## Confirmed UI problems and changes

Moving between games on Home replaced the hero and then called `fitAll()` for the entire TV
root. Each unchanged title was reset, measured and shrunk through repeated style writes and
layout reads. Home includes rows of detected games, so more matched games meant more work
on every movement. The fix fits only the changed hero. A failed logo also fits only its
containing element when its text fallback appears.

The UI filtered repeated controller buttons using their JavaScript arrival time, dropping a
second event within 70 ms. The Rust controller reader already filters duplicates before
emitting events. If the UI was blocked, distinct native presses could arrive together and be
discarded. The UI now preserves native events and retains duplicate filtering for the polled
browser fallback. In the regression fixture, the old code delivered only one of four queued
direction events; the fixed code delivers all four.

Horizontal row navigation queried and filtered every focusable element on the page before
taking its row-specific path. That query now happens only when spatial navigation needs it.
Old lazy-grid registrations also retained their lists across redraws; the registrations are
now cleared when the page is rebuilt.

## WebKit timing comparison

An isolated headless GTK WebKit2GTK 4.1 fixture used the repository's CSS, real `focus()` and
`fitName()` implementations, a 1920 × 1080 window, and synthetic long game names. Both renderer
flags above were set. Image decoding, networking and scrolling were stubbed out to isolate
synchronous title fitting. Each sample changed focus six times, discarded the first two
measurements and reported the median of the remaining four.

| Titles in the Home row | Before | After |
| --- | ---: | ---: |
| 12 | 9 ms | 3 ms |
| 103 | 205 ms | 3 ms |

These are synchronous handler timings, not end-to-end input latency or desktop frame-rate
measurements. The original larger fixture exceeded its 45-second timeout. The 103-title
fixture establishes that the repeated fitting can exceed the controller's roughly 120 ms
repeat interval, but does not establish the exact cost of the player's real game names and art.

The automated regression suite exercises the real controller listener and navigation functions
with a DOM stand-in: `node --test tests/tv-performance.test.cjs`. It checks queued input
delivery and the scope of DOM queries; it does not assert wall-clock timings.

## Remaining graphics and scale questions

`src-tauri/src/sysenv.rs` selects the conservative renderer for NVIDIA in Auto mode and also
disables compositing under Wayland. This matches the running WebKit child's flags. The code
documents blank-window problems that motivated those defaults. They were left unchanged;
the benchmark does not compare renderer modes or establish that Faster works on this system.
The macOS launch path does not set these Linux flags, and a smaller Home page also avoided
much of the repeated fitting work.

Cover Flow still creates cases and cover/reflection images for the whole library, and visits
all cases on each animation frame even though only nearby cases are painted. Grid rendering
initially limits its rows, but retains rows as they are added. Neither was virtualized in this
change. Full TV redraws and spatial up/down navigation also still scale with page content.

For live follow-up, compare the same Home row and rapid controller taps in the rebuilt app;
then compare Lighter Launcher on/off. A separate test of Graphics Mode under desktop Settings
→ Appearance requires restarting the app. Faster may restore the blank-window problem, so a
renderer-mode comparison needs an actual visible-window check before changing Auto defaults.
