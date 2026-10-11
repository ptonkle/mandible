# Triage

Stage 7, written by the person after using the build. `C4 Verify against hand-computed cases`.

One row per concrete change. Say what you did, what you expected, and what happened. Compare at least one result with a case you worked out by hand.

| # | What I did | Expected | What happened | Change wanted | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | Clicked the grid tab headers (Ants, Build, Waypoints, …) | The tab opens on every click | It does not open reliably | Tab clicks always work. *Agent's likely cause: the HUD is rebuilt about 10 times a second, so a button replaced between mouse-down and mouse-up never receives the click. Grid buttons and queue icons are probably affected too.* | open |
| 2 | — | — | — | A sandbox mode. *Details to decide: for example unlimited resources, every tier unlocked, spawning ants for any colony, a passive or no opponent, and pause or speed control.* | open |
| 3 | Hovered an ant's button on the Build tab | The tooltip stays steady | Now and then the tab header buttons flash on top of the tooltip | No flicker. *Agent's likely cause: the same rebuild as row 1. Each rebuild replaces the hovered button, so the tooltip is briefly hidden and redrawn. Fix rows 1 and 3 together.* | open |
| 4 | Hovered things in the top bar (queue icons, resource and Income hints) | A readable tooltip | The tooltip goes off the top of the window | Tooltips always stay inside the window. *Agent's note: tooltips are always drawn above what they point at. They should flip below when there is no room above, and shift sideways at the screen edges.* | open |
| 5 | Trained several ants of one species | One queue icon per species per queen or hatchery | One icon per ant | Group the queue bar: for each queen or hatchery, one icon per species with a count on it (for example Soldier Ant ×10), the front item's progress fill, and clicking cancels one. | open |
| 6 | Looked at the All Ants button (Waypoints tab) | A clear label | An ant picture, the same as Soldier Ant | Show the text "ALL" in place of the picture. *Agent's note: "ALL" is short for the glossary term All Ants, so the glossary row gets a note that "ALL" is the button's short form.* | open |
| 7 | Hovered Queen and Brood Cell on the Build tab | An Effect row, as hatcheries have ("Effect: Train Acid Ant") | No Effect row | Queen: Effect: Train Harvester Ant, Soldier Ant, Carpenter Ant · Drop-off · Housing +20. Brood Cell: Effect: Housing +15. *Agent's reading: "effects" means the tooltip's Effect row, not visual effects. To confirm.* | open |
| 8 | Looked at the Build tab | A quick way to see what each structure needs | The tier is only in the tooltip | A small tier number (1, 2 or 3; digits, as the glossary says for tiers) on every Build button. *Agent's suggestion, to confirm: the same badge on the Ants and Upgrades buttons, for consistency.* | open |
| 9 | — | — | — | An auto-scout feature. *Details to decide: a toggle on the Colony tab; while on, one ant (perhaps the cheapest combat ant, or a Fire Ant for speed) visits unexplored chambers on its own, nearest first, then sweeps chambers out of sight. The AI already has this logic. New glossary term needed (for example Auto-Scout).* | open |
