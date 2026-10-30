#!/usr/bin/env python3
"""The installer's pictures: each screen's background (with its words and button drawn in), at the
five Windows scale sizes. Run: python3 make_art.py  -> ../bmp/<name>@<scale>.bmp"""
import asyncio, os, subprocess
from playwright.async_api import async_playwright
HERE = os.path.dirname(os.path.abspath(__file__))
UI = os.path.abspath(os.path.join(HERE, '../../../ui'))
OUT = os.path.join(HERE, '..', 'bmp')
W, H = 720, 460
SCALES = [100, 125, 150, 175, 200]
CSS = f"""
@font-face{{font-family:Archivo;font-weight:100 900;font-stretch:62% 125%;src:url(file://{UI}/fonts/archivo-wdth.woff2) format("woff2")}}
@font-face{{font-family:Figtree;font-weight:500;src:url(file://{UI}/fonts/figtree-500.woff2) format("woff2")}}
@font-face{{font-family:Figtree;font-weight:600;src:url(file://{UI}/fonts/figtree-600.woff2) format("woff2")}}
@font-face{{font-family:Figtree;font-weight:700;src:url(file://{UI}/fonts/figtree-700.woff2) format("woff2")}}
*{{box-sizing:border-box;margin:0}}
html,body{{width:{W}px;height:{H}px;overflow:hidden;background:#0B1220}}
body{{position:relative;font:500 15px Figtree,sans-serif;color:#F4F8FC}}
.glow{{position:absolute;inset:0;background:radial-gradient(60% 55% at 12% -8%,rgba(52,242,232,.16),transparent 70%),radial-gradient(55% 60% at 98% -12%,rgba(55,123,182,.38),transparent 70%)}}
.edge{{position:absolute;inset:0;border:1px solid rgba(244,248,252,.10)}}
.x{{position:absolute;right:14px;top:12px;width:32px;height:32px;display:grid;place-items:center;color:#8B97AA}}
.c{{position:absolute;left:0;right:0;display:flex;justify-content:center}}
.mark{{width:76px;height:76px}}
.wm{{height:25px}}
.lead{{font:600 16px Figtree,sans-serif;color:#A9B4C4}}
h1{{font:800 30px/1 Archivo,sans-serif;font-stretch:112%;letter-spacing:-.02em}}
.btn{{height:52px;padding:0 34px;border-radius:13px;background:#34F2E8;color:#06121A;font:700 17px Figtree,sans-serif;display:flex;align-items:center;gap:10px;box-shadow:0 14px 30px -14px rgba(52,242,232,.8)}}
.foot{{font:500 12px Figtree,sans-serif;color:#6f7d90}}
.btn.ghost{{background:rgba(244,248,252,.08);color:#F4F8FC;box-shadow:none;border:1px solid rgba(244,248,252,.14)}}
.btn.red{{background:#FF6B6B;color:#1A0A0A;box-shadow:0 14px 30px -14px rgba(255,107,107,.7)}}
.pair{{gap:16px}}.pair .btn{{width:176px;justify-content:center;padding:0}}
.box{{width:20px;height:20px;border-radius:6px;border:1.6px solid #6f7d90;display:grid;place-items:center}}
.box.on{{background:#34F2E8;border-color:#34F2E8;color:#06121A}}
"""
X = '<svg width="16" height="16" viewBox="0 0 16 16"><path d="m3 3 10 10M13 3 3 13" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>'
ARROW = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14"/></svg>'
WRENCH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a4 4 0 0 0-5.4 5.4L4 17l3 3 5.3-5.3a4 4 0 0 0 5.4-5.4l-2.6 2.6-2.4-.6-.6-2.4z"/></svg>'
TICK = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.4" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5"/></svg>'
PLAY = '<svg width="16" height="16" viewBox="0 0 24 24"><path d="M7 4.5v15l12-7.5z" fill="currentColor"/></svg>'
def page(inner):
    return f'<!doctype html><html><head><style>{CSS}</style></head><body><div class="glow"></div>{inner}<div class="edge"></div></body></html>'
LOGO = f'<div class="c" style="top:56px"><img class="mark" src="file://{HERE}/mark.svg"></div><div class="c" style="top:148px"><img class="wm" src="file://{HERE}/wordmark.svg"></div>'
SHOTS = {
  # The first screen: Install (or Update). The location line and Change sit under the button as live text.
  'welcome': page(LOGO + f'<div class="c lead" style="top:190px">One home for PC recomps</div><div class="c" style="top:258px"><div class="btn">{ARROW}Install</div></div><div class="c foot" style="top:424px">Free software under the GPL-3.0 · No game files included</div><div class="x">{X}</div>'),
  'update': page(LOGO + f'<div class="c lead" style="top:190px">A new version is ready</div><div class="c" style="top:258px"><div class="btn">{ARROW}Update</div></div><div class="c foot" style="top:424px">Your games, saves and settings stay as they are</div><div class="x">{X}</div>'),
  # Already installed, the same version: put its files back. Uninstall sits under it as live text.
  'repair': page(LOGO + f'<div class="c lead" style="top:190px">Already installed</div><div class="c" style="top:258px"><div class="btn">{WRENCH}Repair</div></div><div class="c foot" style="top:424px">Puts its files back. Your games, saves and settings stay as they are</div><div class="x">{X}</div>'),
  # The uninstaller: are you sure (the "also delete" choice under the buttons is live), then removing, then done.
  'unconfirm': page(f'<div class="c" style="top:56px"><img class="mark" src="file://{HERE}/mark.svg"></div><div class="c" style="top:156px"><h1>Uninstall RecompLauncher?</h1></div><div class="c lead" style="top:202px">Your games and their saves stay in their folders.</div><div class="c pair" style="top:258px"><div class="btn ghost">Keep it</div><div class="btn red">Uninstall</div></div><div class="x">{X}</div>'),
  'unprogress': page(f'<div class="c" style="top:74px"><img class="mark" src="file://{HERE}/mark.svg" style="width:64px;height:64px"></div><div class="c" style="top:166px"><h1>Uninstalling</h1></div>'),
  'unbye': page(f'<div class="c" style="top:56px"><img class="mark" src="file://{HERE}/mark.svg"></div><div class="c" style="top:156px"><h1>RecompLauncher is uninstalled</h1></div><div class="c lead" style="top:202px">Your games are still in their folders, whenever you want them.</div><div class="c" style="top:258px"><div class="btn ghost" style="width:176px;justify-content:center;padding:0">Close</div></div><div class="x">{X}</div>'),
  # Installing: the heading here, the status and the bar are live.
  'progress': page(f'<div class="c" style="top:74px"><img class="mark" src="file://{HERE}/mark.svg" style="width:64px;height:64px"></div><div class="c" style="top:166px"><h1>Installing</h1></div>'),
  'done': page(f'<div class="c" style="top:56px"><img class="mark" src="file://{HERE}/mark.svg"></div><div class="c" style="top:156px"><h1>You\'re all set</h1></div><div class="c lead" style="top:202px">RecompLauncher is installed.</div><div class="c" style="top:258px"><div class="btn">{PLAY}Open RecompLauncher</div></div><div class="x">{X}</div>'),
}
# Small pictures: the uninstaller's tick box, off and on.
SMALL = {
  'chk-off': (20, 20, '<div class="box"></div>'),
  'chk-on': (20, 20, f'<div class="box on">{TICK}</div>'),
}
ONLY = os.environ.get('ONLY', '').split(',') if os.environ.get('ONLY') else None
async def main():
    os.makedirs(OUT, exist_ok=True)
    async with async_playwright() as p:
        b = await p.chromium.launch()
        for s in SCALES:
            pg = await b.new_page(viewport={'width': W, 'height': H}, device_scale_factor=s / 100)
            for name, html in SHOTS.items():
                if ONLY and name not in ONLY: continue
                f = os.path.join(HERE, f'_{name}.html'); open(f, 'w').write(html)
                await pg.goto('file://' + f); await pg.wait_for_timeout(300)
                png = os.path.join(OUT, f'{name}@{s}.png')
                await pg.screenshot(path=png)
                subprocess.run(['convert', png, '-type', 'TrueColor', 'BMP3:' + png[:-4] + '.bmp'], check=True)
                os.remove(png); os.remove(f)
            await pg.close()
            for name, (w, h, inner) in SMALL.items():
                if ONLY and name not in ONLY: continue
                pg = await b.new_page(viewport={'width': w, 'height': h}, device_scale_factor=s / 100)
                html = f'<!doctype html><html><head><style>{CSS}html,body{{width:{w}px;height:{h}px}}</style></head><body>{inner}</body></html>'
                f = os.path.join(HERE, f'_{name}.html'); open(f, 'w').write(html)
                await pg.goto('file://' + f); await pg.wait_for_timeout(200)
                png = os.path.join(OUT, f'{name}@{s}.png')
                await pg.screenshot(path=png)
                subprocess.run(['convert', png, '-type', 'TrueColor', 'BMP3:' + png[:-4] + '.bmp'], check=True)
                os.remove(png); os.remove(f)
                await pg.close()
        await b.close()
asyncio.run(main())
