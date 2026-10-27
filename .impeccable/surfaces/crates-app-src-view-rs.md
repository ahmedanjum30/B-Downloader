---
version: 1
slug: "crates-app-src-view-rs"
primary_target: "crates/app/src/view.rs"
related_targets: ["crates/app/src/dialogs.rs","crates/app/src/style.rs"]
---

## Surface

The main window and its dialogs (crates/app/src/view.rs, dialogs.rs, style.rs). Mode: Operate. Everyday Windows users first, power users second; the job is to glance at downloads, add one, and handle a failed or queued one.

## Direction contract

THESIS: A shadcn/ui-style working screen (user-pinned): flat zinc-slate surfaces, 1px borders and quiet hierarchy, refusing the category's glowing gradient dashboard and the busy IDM grid.
OWN-WORLD: Slate greys with one blue. Dark: #020617 ground, #0F172A cards, #1E293B borders, #F8FAFC text, #94A3B8 muted, primary #3B82F6. Light: #FFFFFF ground and cards, #E2E8F0 borders, #0F172A text, #64748B muted, primary #2563EB. Radius 6px controls, 10px cards and dialogs. Outline and ghost buttons; segmented tabs on a muted track; status as small dot badges.
STORY: The visitor sees what's running and how fast, what's waiting, and what needs them, then acts with one obvious button.
FIRST VIEWPORT: Left sidebar (All downloads, Queue, Types) on the ground colour with a 1px divider; header with title and speed subtitle, then the speed limit, when-done, search and the blue Add download; status tabs; bordered download rows with the per-connection progress bar.
FORM: shadcn/ui component language over slate + blue; user-pinned, overriding the roll. seed:5b6ff9e9fc9df6e8
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
