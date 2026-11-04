# B-Downloader design

The desktop app (`crates/app`, iced) follows AB Download Manager's world: charcoal panels, one blue-to-violet gradient, and a list grouped by what needs the user.

## Colour

Tokens live in `crates/app/src/style.rs` (`Colors`, `DARK`, `LIGHT`).

| Token | Dark | Light | Use |
|---|---|---|---|
| background / sidebar | #1c1c21 | #f4f4f6 | Window ground |
| surface | #24242b | #ffffff | Panels, inputs, group cards |
| surface_hover | #2c2c34 | #ebebef | Hover, raised controls |
| border | #34343d | #dcdce2 | Dividers and outlines |
| text | #e6e6ea | #1c1c21 | Body text |
| muted | #8d8d98 | #66666f | Secondary text, queued state |
| accent → accent_end | #4a8df6 → #a35bf7 | #3b7ce8 → #9147ee | The one gradient |
| success | #3fb950 | | Finished parts and downloads |
| warning | #e3a008 | | Paused |
| danger | #f85149 | | Failed |

The gradient (`style::gradient`, 90°) appears on only three things: running progress bars, the Add URL tile, and the primary button's fill. Everything else stays grey, so the gradient always means "moving" or "the main action".

## Shape

- Controls have an 8px radius (`RADIUS`); panels and group cards have 12px (`CARD_RADIUS`).
- The sidebar has no border. A 1px rule separates it from the main panel.

## Layout

- **Sidebar:** a card holding All downloads and its five categories, a second card holding Queue and Torrents, and a footer with Browser extension, Settings and Support.
- **Toolbar:** the Add URL pill, then icon-over-label buttons (Resume all, Pause all, Start/Stop queue, Settings), with search on the right.
- **List:** grouped by state, never by tabs.
  - "Downloading now" uses tall rows. Each has a 6px gradient bar, plus a 3px strip under it showing each connection's part, with finished parts in green.
  - "Needs you", "Queued" and "Finished" are compact rows inside group cards, with columns for name and type, state, size, and actions. A state shows its reason only when the download failed or is seeding.
- **Status bar:** speed limit and When done on the left; downloading and queued counts and total speed on the right.

## Dialogs

Dialogs share the same tokens. Inputs sit on surface, choices use segmented controls, and the primary action uses the gradient fill.
