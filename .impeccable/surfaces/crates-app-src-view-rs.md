---
version: 1
slug: "crates-app-src-view-rs"
primary_target: "crates/app/src/view.rs"
related_targets: ["crates/app/src/dialogs.rs","crates/app/src/style.rs"]
---

## Surface

The main window and its dialogs (crates/app/src/view.rs, dialogs.rs, style.rs). Mode: Operate. Everyday Windows users first, power users second; the job is to glance at downloads, add one, and handle a failed or queued one.

## Direction contract

THESIS: A working download manager in AB Download Manager's world (user-pinned): charcoal panels and one blue-to-violet gradient, with the list grouped by what needs the user instead of one flat list or a busy IDM grid.
OWN-WORLD: Charcoal greys and one gradient. Dark: #1c1c21 ground, #24242b panels, #2c2c34 raised, #34343d borders, #e6e6ea text, #8d8d98 muted; gradient #4a8df6 to #a35bf7 on progress, the Add URL tile and the primary button's fill (translated from the mock's gradient outline: iced draws no gradient borders, so the fill carries it); green #3fb950 done parts, amber #e3a008 paused, red #f85149 failed. Light keeps the structure on #ffffff / #f4f4f6 with the same gradient. Radius 8px controls, 12px panels. Icon-over-label toolbar buttons; a rounded category card; compact table groups (name, state, size, actions).
STORY: The visitor sees what's downloading and how fast, what needs them, and what waits, then acts on the row in front of them.
FIRST VIEWPORT: Category card left (All with its types, then Queue and Torrents); toolbar on top with the Add URL pill and icon-over-label Resume all, Pause all, Start queue, Settings, plus search; "Downloading now" tall rows with gradient bar and per-connection strip, then "Needs you", "Queued" and "Finished" as compact table groups; status bar at the bottom: speed limit and When done on the left, counts and total speed on the right.
FORM: AB Download Manager style (user-pinned), the grouped layout, candidate 6 of 7 dealt as the lead. seed:73174734da3eecf5
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
