# 0006. Visual direction and design system

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

[Principle P4](../principles.md#p4--design-before-code) asks for a validated visual
direction before the UI is built, and for the UI to rest on an established design
system rather than one invented from scratch.

The UI framework is not chosen yet: the [tech stack](https://github.com/lorenzotricaricoo/Trama/issues/16)
is decided after the spikes. The candidates are a Rust core with a web UI (Tauri) and
a Rust core with a native UI per platform. The design system must therefore work with
either.

Three directions were compared on the Home screen (glass window, sidebar on a frame
with content on a sheet, dark and dense with one accent). The maintainer chose a
combination and validated it on the main MVP screens and on a clickable, animated
prototype.

## Decision

The design system has two layers.

**1. Trama's layer: the visual direction and its tokens.** Described in
[Visual direction](../design/visual-direction.md), with the values in
[`tokens.css`](../design/tokens.css):

- a glass frame over a wallpaper tinted by the accent, with the sidebar on the glass
  and the content on a sheet;
- an accent chosen by the user, as on macOS (System or one of nine colours, lime by
  default), all with the same lightness and chroma; red reserved for recording;
- dark, light and System themes;
- Geist and Geist Mono; Phosphor icons;
- motion tokens (durations, curves, springs) and the motion patterns of the app;
- Reduce motion and Reduce transparency, following the system and settable in the app.

Tokens are defined in OKLCH and are platform-neutral: they carry over to CSS or to
native colour and font definitions unchanged.

**2. The foundation: an established system for component behaviour.** Menus,
popovers, dialogs, focus management, keyboard navigation and accessibility come from
an established system and are styled with Trama's tokens:

| UI stack chosen in the stack ADR | Foundation |
|----------------------------------|------------|
| Web UI (for example Tauri) with React | **React Aria Components** |
| Web UI with another framework | **Ark UI** |
| Native UI per platform | The platform's own controls: SwiftUI/AppKit on macOS, WinUI 3 (Fluent) on Windows |

The stack ADR confirms the row that applies.

## Consequences

- The look of Trama is fixed before implementation, and the tokens are the single
  source for colours, type, spacing and motion.
- Accessibility and keyboard behaviour are not reimplemented by hand; the foundation
  provides them.
- The accent system keeps contrast constant whatever colour the user picks.
- The glass look depends on translucency and blur; with Reduce transparency, or where
  the platform cannot blur behind the window, surfaces fall back to opaque values
  that are already defined.
- Fonts and icons are open source and are bundled with the app, not loaded from the
  network.
- The prototype is kept in a private design tool; the repository documents are the
  durable record.

## Alternatives considered

- **Use an existing look unchanged** (for example the default shadcn/ui or Fluent
  theme): faster, but Trama would look like a generic app, and the accent and glass
  frame chosen by the maintainer would not be possible.
- **A fully custom component library**: complete control over the look, but focus,
  keyboard and screen-reader behaviour would have to be written and tested by hand,
  against P4.
- **Radix UI with shadcn/ui** as the web foundation: popular and well documented,
  but its components ship with their own styling conventions, and React Aria
  Components offers more on internationalization (dates, numbers, right-to-left),
  which [A5](../principles.md#a5--foundations-from-day-one) requires.
