# ADR-0021: Interface colored by the connection state (direction B)

- **Status:** Accepted. Phase 5 of the post-0.2.0 program (steps 41 and 42).
- **Date:** 2026-11-17

## Context

The maintainer asked for a better-looking app. Phase 5a reviewed the
interface with screenshots of the real app (`scripts/ui-review/`: the app with
fake IPC, headless Firefox, both languages and themes) and offered three
directions in a local gallery: A, native GNOME look; B, the screen colored by
the connection state; C, compact and technical. The maintainer chose **B**.

Most people who use the app got the profile from a colleague and only want to
know one thing: is the VPN on?

The review also found problems that any direction had to fix:

- contrast in the light theme below WCAG AA (white on the primary button
  4.28:1, "Preparing…" in amber 2.18:1, "Connected" in green 3.26:1);
- no visible keyboard focus on the hand-made buttons;
- the dialog close button said "Fechar" in English, and the diagnostic levels
  were read as "ok/warn/fail" in Portuguese;
- animations ignored "reduce motion";
- the title wrapped to two lines in the header;
- errors mixed what happened and what to do in one technical block;
- first use had two competing primary actions.

## Decision

- **The top of the connection screen carries the state.** A rounded block
  ("hero") colored by state: slate when disconnected, amber while connecting
  or reconnecting, green when connected, red when the last attempt failed.
  It holds the brand, the profile picker, an icon, the state in large type,
  one sentence about it and **the one action to take now** (Connect,
  Cancel, Disconnect, Enter credentials, Install/Update the network
  component).
- **Below it, the details:** first-use steps (numbered when there are two),
  the error card (what happened, what to do, technical details folded), the
  connection in big tiles (virtual IP, time, received, sent) and the log.
- **Errors are split** at the first ": " of the message from Rust: the probable
  causes already come as "what happened: what to check".
- **Tabs at the bottom** (Connection, Profiles, System), freeing the top; the
  Profiles and System pages get a title.
- **Tokens** in `src/index.css`: a new palette, light and dark, where every
  text/background pair is at least 4.5:1, and one gradient per state.
- **Accessibility:** visible focus on every control, translated labels,
  `prefers-reduced-motion` stops spinning and transitions, the state is a
  heading and changes are announced (`aria-live`).

## Consequences

- The main screen says the state before anything is read, and the next step
  is always the big white button.
- The interface changed for current users: the round button and the top tabs
  are gone. The guides mention the colors.
- `scripts/ui-review/shoot.sh` stays: it is how to see the interface in both
  languages and themes, and at 380 px, before merging a UI change.

## Alternatives considered

- **A, native GNOME:** the smallest change and the most native look, but the
  state shows only in an icon and a word.
- **C, compact and technical:** good for network people, less welcoming for
  whoever got the profile from a colleague.
