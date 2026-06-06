# 0004 — `theme = "auto"` via terminal background (OSC 11)

## Context
We want a sensible default theme without configuration. The OS light/dark setting is the
wrong signal: the terminal may use a different theme, or the session may be over SSH. What
actually governs readability is the **terminal's background color**.

## Decision
Default `theme = "auto"`. At startup, **before** entering raw mode / the alt-screen, query
the terminal background via **OSC 11** (using `termbg`), fall back to `COLORFGBG`, then to
dark. Map the result to a paired (syntect theme + diff palette): dark or light. A specific
theme name in config disables detection.

## Consequences
- Good out-of-the-box contrast in most terminals, no config needed.
- The query must happen once, early, before terminal mode changes (ordering matters).
- Some terminals don't answer OSC 11 → graceful fallback to dark.
- macOS `AppleInterfaceStyle` is at most a secondary fallback, not the primary signal.
