# Triage

Stage 7, written by the person after using the build. `C4 Verify against hand-computed cases`.

One row per concrete change. Say what you did, what you expected, and what happened. Compare at least one result with a case you worked out by hand.

| # | What I did | Expected | What happened | Change wanted | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | Clicked the grid tab headers (Ants, Build, Waypoints, …) | The tab opens on every click | It does not open reliably | Tab clicks always work. *Agent's likely cause: the HUD is rebuilt about 10 times a second, so a button replaced between mouse-down and mouse-up never receives the click. Grid buttons and queue icons are probably affected too.* | open |
| 2 | — | — | — | A sandbox mode. *Details to decide: for example unlimited resources, every tier unlocked, spawning ants for any colony, a passive or no opponent, and pause or speed control.* | open |
| 3 | Hovered an ant's button on the Build tab | The tooltip stays steady | Now and then the tab header buttons flash on top of the tooltip | No flicker. *Agent's likely cause: the same rebuild as row 1. Each rebuild replaces the hovered button, so the tooltip is briefly hidden and redrawn. Fix rows 1 and 3 together.* | open |
| 4 | Hovered things in the top bar (queue icons, resource and Income hints) | A readable tooltip | The tooltip goes off the top of the window | Tooltips always stay inside the window. *Agent's note: tooltips are always drawn above what they point at. They should flip below when there is no room above, and shift sideways at the screen edges.* | open |
