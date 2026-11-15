# Game Media and Cover Labels in the Catalog

Two optional parts of the catalog decide how a game looks in the launcher. Both are made on the
website, so changing them never needs a new launcher.

## `media_design` (per game)

The cartridge, card or disc shown while a game sets up with Easy Setup. The launcher draws the
console's own media (`ui/media-art.js`) and puts the game on it as this object says. Every field is
optional; a game without `media_design` gets the defaults. (A game's `media` is something else: its
"Around the web" links.)

| Field | Meaning |
|---|---|
| `kind` | Which media to draw, if not the console's own: `switch`, `switch2`, `vita`, `nes`, `ds`, `3ds`, `gba`, `gb`, `gbc`, `snes`, `n64`, `genesis`, `umd`, `gc`, `bluray`, `dvd`. |
| `art` | What fills the label: `cover`, `hero` (the backdrop), `url` (an image of its own) or `scan` (a photo of the real disc face or cartridge label, used whole). |
| `src` | The image for `url` and `scan`. |
| `style` | `image`, `blur` or `color`. Discs default to `color`. |
| `crop` | `{ x, y, z, w, h }`: where the image is cut. `x` and `y` run from 0 (left, top) to 1 (right, bottom) of what overflows the label, `z` zooms in (1 fills the label), `w` and `h` are the image's size in pixels. |
| `color` | `#rrggbb`, the print colour for `color`. |
| `logo` | `{ on, x, y, s, a }`: the logo's centre (0 to 1 across the label's box), its width as a share of the box's, and its width / height. |

Defaults: cartridges and cards show the cover (the wide GBA and SNES labels show the backdrop with
the logo); discs and the UMD show a colour with the logo in the band above the hub; the small
GameCube disc has no logo. Consoles without media of their own get a DVD-sized disc.

## `labels` (top level)

Console Labels on Covers, which players turn on in Settings → Library: a strip across the top of
each cover with its console, like the band on a real game box.

```json
"labels": {
  "x360": { "src": "https://…/label-x360.png", "s": 1, "y": 0, "a": 6.67, "h": 0.1, "push": false }
}
```

Keys are console ids (a game's `origin`). `s` is the label's width as a share of the cover's,
`y` moves it down as a share of the cover's height, `a` is the PNG's width / height, `h` its height
as a share of a 2:3 cover's (the launcher moves the corner badges under it), and `push` starts the
cover's art under the label instead of behind it.
