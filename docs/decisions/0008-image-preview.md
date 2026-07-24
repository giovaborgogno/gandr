# 0008 — Image preview via `ratatui-image` (protocol-detected, halfblocks fallback)

## Context
DESIGN.md §4 says binary/image files "render a placeholder (`binary file, N bytes`)".
For a tool built to review AI-coding-agent changes, that leaves a real gap: agents commit
icons, screenshots, generated assets, and diagrams, and today you can't *see* them — only
a byte count. We want to preview raster images (and later SVGs) inline, in both the diff
viewer (a changed image) and the Repo browser (any image file).

Terminals render images through several mutually-incompatible mechanisms. Surveying the
field (yazi, viu, chafa, timg): the standard is a **detect-and-fallback** stack — native
graphics protocols where present (Kitty, iTerm2/WezTerm, Sixel), degrading to Unicode
half-blocks (two colored pixels per cell) everywhere else. yazi additionally shells out to
`ueberzug++`/`chafa`; that's an external-process model we don't want (see ADR 0003 — we
don't shell out for core rendering).

Two build-vs-buy options:
1. Hand-roll the escape sequences + terminal capability queries ourselves.
2. Use **`ratatui-image`** — the ratatui-native widget for exactly this. It queries the
   terminal for protocol support + font-size, maps image pixels to the cell grid, and emits
   the protocol escapes *through ratatui's buffer* by marking covered cells as "skip" (so
   ratatui's diffing renderer doesn't paint over the image). It depends on `ratatui 0.30`
   and `image 0.25` — both already compatible with our stack.

## Decision
1. **Use `ratatui-image` as the rendering layer.** It integrates with our existing
   `frame.render_widget` model (the skip-cell trick) rather than fighting the dirty-flag
   render loop with hand-emitted escapes. Protocol precedence is the crate's default:
   Kitty → iTerm2 → Sixel → **half-blocks fallback**. Detection runs **once at startup**
   (`Picker::from_query_stdio`), sequenced with the existing `termbg`/OSC 11 query so the
   two stdin probes don't race.
2. **Half-blocks is the baseline, and the test target.** Half-blocks render as ordinary
   colored cells in ratatui's buffer, so they appear in `TestBackend` snapshots — which
   makes image preview verifiable headlessly (golden rule #1). Snapshot tests force
   `Picker::halfblocks()`; the native protocols are opt-in-by-detection at runtime only.
3. **Raster now, SVG later (behind a feature flag).** v1 of this feature handles the
   `image` crate's formats (PNG/JPG/GIF/WebP/BMP). SVG (rasterize with `resvg`/`usvg`/
   `tiny-skia`, then feed the same pipeline) lands in a later milestone gated by a Cargo
   feature, because that dependency tree is heavy and slow to compile.
4. **Decode *and* encode off the render thread.** Both the decode (O(pixels)) and the
   resize+encode to the pane's cell area are expensive — measured at ~16ms and ~19ms
   respectively for a 1600×1200 image at a full-screen pane. Doing the encode inline in
   `render` (the obvious first cut) drops ~20ms frames while scrolling over images at
   full-screen — small panes hid it (~5ms). So the job (`spawn_image`, on the existing
   crossbeam worker pool from ADR 0006 / M13 — *not* tokio) does **both**: it decodes and
   builds a static `Protocol` already sized to the pane, via a cheap `Picker` clone. The
   pane's cell area is only known at render, so `render` records it (`image_wanted_area`)
   and the job is spawned for that size; rendering then just re-emits the ready protocol
   (~130µs even full-screen). A terminal resize changes the recorded area, which re-spawns
   the job for the new size; a stale result (selection moved, or old size) is dropped by
   epoch. This is simpler than `ratatui-image`'s `ThreadProtocol` (which shuttles a
   `StatefulProtocol` to a worker and back over an mpsc channel) and keeps `render`
   non-blocking and snapshot-testable. A dedicated byte cap (`MAX_IMAGE_BYTES`, separate
   from `MAX_DIFF_BYTES`) bounds decode work/memory. Decoding runs eagerly as you scroll
   (one job at a time) but the image is only *transmitted* to the terminal once the selection
   settles (a short debounce) — otherwise scrolling a folder of images sends a full graphics
   escape per step, which the terminal must render, and it stutters. A small LRU cache of
   decoded previews (keyed by file + pane area) makes scrolling back instant.
5. **Read-only, no new external binaries.** All decoding is in-process (no `chafa`/
   `ueberzug`/`ffmpeg` subprocess). Consistent with ADR 0003 and the read-only invariant.
6. **Escape hatch.** A config/CLI switch disables image rendering (force the placeholder),
   and detection failure degrades silently to half-blocks, then to the byte-count
   placeholder if even that isn't viable.

## Consequences
- DESIGN.md §4 changes: binary *image* files now render the image (protocol-detected),
  falling back to the `binary file, N bytes` placeholder for non-images, undetectable
  terminals with `images = false`, or files over the image cap. Updated alongside this ADR.
- The UI gains access to raw blob bytes it doesn't have today: the diff engine discards
  binary content (`FileDiff` carries only decoded `String`s), so image bytes are fetched
  on demand via `GitBackend::file_contents` through the job system (`spawn_image`) and
  decoded off-thread — never retained on the hot diff path. Lightweight metadata (format +
  dimensions) is probed eagerly where the bytes already exist (M15a); the full decode is
  lazy and async (M15b).
- New dependencies: `ratatui-image` + `image` (raster). SVG adds `resvg`/`usvg`/`tiny-skia`
  under a default-off feature.
- New gotcha class: inline-graphics escapes vs. the dirty-flag redraw loop and scrolling —
  images must be cleared when the previewed file changes (explicit delete on Kitty) and
  re-emitted on redraw. This is verified per-terminal manually; half-blocks is verified in
  snapshots.
- Does **not** change the non-goals: still a viewer. No editing, no annotation of images.
