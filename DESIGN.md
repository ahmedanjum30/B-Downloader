# B-Downloader design

The desktop app (`crates/app`, iced) follows AB Download Manager's world: charcoal panels, one blue-to-violet gradient, and a list grouped by what needs the user.

## How design work is done

All design work (new screens, themes, redesigns, mockups and variants to choose from) uses both the **impeccable** and **ui-ux-pro-max** skills together. Show proposals in impeccable's live server for review before any change to the app's code.

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

### Hand-drawn themes

Two more looks, added and picked in Settings > Themes, sit beside Dark and Light. Dark and Light come with the app; these download their fonts when added (`themes.rs`, files in `themes/`, published on GitHub Pages) (`GRAPHITE`, `CHALK` in `style.rs`; mockups in `.impeccable/mocks/screens/sketch.html`). Each brings its own fonts, line weight and corners; layout and behaviour stay the same.

| Token | Graphite | Chalk | Use |
|---|---|---|---|
| background | #f4efe1 cream paper | #262b28 slate | Window ground |
| surface | #fbf8ef | #2b312d | Cards, inputs |
| border | #2b2a26 graphite | #93948d chalk at 55% | Drawn outlines, 1.6px / 1.5px |
| text / muted | #2b2a26 / #5c584e | #ece9df / #b9b7ac | |
| accent | #2b57a6 ballpoint | #8cc4f0 chalk blue | Solid, no gradient |
| fonts | Patrick Hand, Kalam Bold | Gochi Hand | Body, then labels and headings |

- Corners differ per corner (Graphite controls 9/4/8/3px, cards 14/5/12/6px), so boxes look drawn rather than ruled.
- Where Dark and Light use a fill, the hand-drawn looks draw an outline instead: the selected sidebar entry, file badges, progress tracks.

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
