# Visual direction and design system

How Trama looks and moves. The screens and flows are described in the
[information architecture](information-architecture.md); the decision behind this
document is [ADR 0006](../adr/0006-visual-direction-and-design-system.md).

## Direction

**A glass frame on the desktop, with the content on a sheet.** The window is a
translucent frame over a wallpaper tinted by the accent colour. The sidebar sits
directly on the glass; the content of each screen lies on an opaque sheet inside the
frame. The result is calm, dense and keyboard-first.

References (inspiration, not copies): **Raycast** for the command bar and keyboard
hints, **Arc** for the sidebar with pinned items and the tinted window, **macOS**
for the accent colour and the system settings for theme, transparency and motion.

How it was chosen:

1. Three directions were compared on the Home screen, in dark and light: *Glass*
   (translucent window), *Frame* (sidebar on the frame, content on a sheet) and
   *Command* (dark, dense, a single strong accent).
2. The maintainer chose a combination: the glass of the first, the frame of the
   second, the colours of the third, with the accent chosen by the user.
3. The combined direction was applied to Home, Meeting, Project, Doubts, the
   recording window and Settings › Appearance, then turned into a clickable,
   animated prototype covering every MVP screen in the
   [information architecture](information-architecture.md#screens).

The prototype lives in a private design tool and is not linked here. This document
and [`tokens.css`](tokens.css) are the durable record.

## Colour

All colours are defined in **OKLCH**, so lightness and contrast are predictable.

### Accent

The user picks the accent, as on macOS: **System** (follows the macOS accent) or one
of nine colours. Every accent has the **same lightness and chroma**; only the hue
changes, so every accent has the same contrast against text and backgrounds.

| Accent | Hue | Accent | Hue |
|--------|-----|--------|-----|
| **Lime** (default) | 128 | Red | 25 |
| Blue | 250 | Orange | 55 |
| Purple | 300 | Yellow | 90 |
| Pink | 350 | Green | 155 |
| Graphite | 270 (chroma 0.012) | | |

- Accent fill: `oklch(0.8 0.17 h)`, with dark text on it.
- Accent text and links: a darker variant in light mode, a lighter one in dark mode.
- The wallpaper behind the frame is tinted with the accent hue.

### Reserved colours

- **Red is reserved for recording** (the record button while recording, the timer,
  Stop). It is never used as an accent for anything else, even when the user picks
  the red accent, which is a lighter, softer red.
- **Project colours** are data, not interface: six fixed hues at the same lightness,
  chosen per project together with its icon.

### Themes

**Dark** and **light**, plus **System** (follows macOS). Both themes use the same
roles; the values are in [`tokens.css`](tokens.css).

| Role | Used for |
|------|----------|
| `wall` | The tinted wallpaper behind the frame |
| `frame`, `fink`, `fmute`, `fhover`, `fsel` | The glass frame and the sidebar on it |
| `sheet`, `card`, `pop` | Content sheet, cards, popovers and menus |
| `panel`, `panel-2`, `edge`, `line` | Tiles, controls, borders, separators |
| `ink`, `mute`, `faint` | Text: primary, secondary, tertiary |
| `acc`, `acc-ink`, `acc-t`, `acc-soft` | Accent fill, text on accent, accent text, accent tint |
| `kbg`, `kink` | Keyboard hint chips |
| `danger` | Recording only |

## Typography

**Geist** for the interface, **Geist Mono** for times, durations, counters, keyboard
hints and citations. Both are open source (SIL Open Font License).

| Style | Size / weight | Used for |
|-------|---------------|----------|
| Title | 26 / 650, tight tracking | Screen titles |
| Body | 13 / 400–500 | Everything else |
| Small | 12 / 500 | Chips, secondary text |
| Label | 11 / 600, uppercase | Section labels |
| Meta | Geist Mono 11.5 | Times, durations, dates |
| Hint | Geist Mono 10.5 | Keyboard hints, citations |

## Layout

| Element | Value |
|---------|-------|
| Frame | Inset 22 px from the window edge, radius 14, 8 px padding and gap |
| Sidebar | 216 px; collapses to a 64 px icon rail (button or ⌘\\) with tooltips |
| Sheet | Radius 10, content padding 28 px |
| Tiles and cards | Radius 10, 14–16 px padding |
| Controls | Radius 7–8; chips and counters fully rounded |
| Spacing | Multiples of 2 px, mostly 4, 8, 12, 14, 18, 28 |

## Icons

**Phosphor** (regular weight, MIT licence). Every sidebar item has an icon. Every
project has an icon and a colour that the user can change from the sidebar.

### App icon

**Ordito**: a swatch of fabric on the dark graphite of the frame, three accent threads
(warp) crossing three light ones (weft), over and under. It is the name taken literally:
different sources woven into one base. It follows the macOS grid (824 px squircle on a
1024 px canvas, soft shadow, light inner edge) and uses no red, which stays for recording.

- The source is `apps/desktop/src-tauri/icons/source/ordito.svg`, in Lime.
- The bundle icon (Finder, Launchpad, Spotlight, the `.dmg` and the Dock of a closed app)
  is always Lime: changing it after install would break the signature.
- While Trama is open the Dock icon follows the accent: the threads and the glow take the
  accent's hue (graphite also its low chroma), one image per accent in `icons/dock/`.
- `python3 scripts/app-icon.py` (from `apps/desktop`, needs `rsvg-convert`) renders the
  bundle sizes with `tauri icon` and the Dock icons from the source.

## Motion

Motion explains what happened: where a screen came from, where a doubt went, where
the recording window went when it was stopped. It is quick and never blocks input.

| Token | Value | Used for |
|-------|-------|----------|
| `d1` | 120 ms | Press, hover |
| `d2` | 200 ms | Fades, colour changes |
| `d3` | 320 ms | Popovers, toasts, small entrances |
| `d4` | 520 ms | Screen transitions, sidebar, accent change |
| `e-out` | `cubic-bezier(.22,1,.36,1)` | Entrances |
| `e-in` | `cubic-bezier(.55,0,.8,.3)` | Exits |
| `e-io` | `cubic-bezier(.65,0,.35,1)` | Loops (shimmer) |
| `e-spring` | Spring with overshoot | Small elements: badges, chips, popovers |
| `e-soft` | Soft spring | Large movements: sidebar, active indicator |

Patterns used across the app:

- **Screens** change with a short fade and slide in the direction of navigation;
  tiles and rows enter one after the other.
- **Sidebar**: the active item has an indicator that slides between items; collapsing
  animates the width and fades the labels.
- **Accent and theme** changes blend instead of switching.
- **Doubts** are a stack of cards: drag right to accept the curator's proposal, left to
  skip, or press 1–5; the card flies off and the next one rises.
- **Recording**: the record button pulses, the levels move live, and on Stop the
  recording window shrinks into the processing indicator in the header.
- **Progress** bars shimmer while work is running; counters count up; closed actions
  are struck through with a drawn line; changes that can be undone show a toast with
  *Undo*.

Only `transform` and `opacity` are animated, so motion stays smooth.

## Accessibility

- **Reduce motion** and **Reduce transparency** follow the macOS settings and can also
  be set in Settings › Appearance. Reduce motion removes every animation; Reduce
  transparency makes the frame, sheet and menus opaque.
- Accents share lightness and chroma, so contrast does not depend on the user's choice.
- Every action has a keyboard path; shortcuts are shown as grey hint chips.
- Focus is always visible (accent ring).
- Text targets at least WCAG 2.1 AA contrast in both themes; this is checked again on
  the implemented UI.
- Colour is never the only signal: recording also shows a timer and a label, project
  colours come with an icon and a name.

## Foundation

The tokens above are Trama's own layer. Behaviour and accessibility of components
(menus, popovers, dialogs, focus management, keyboard navigation) come from an
established system, chosen together with the UI framework; see
[ADR 0006](../adr/0006-visual-direction-and-design-system.md).
