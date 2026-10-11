# Handoff

Stage 9, written by the agent. `C5 Consolidate in your own words`.

A fresh session starts from this file, not from memory.

## What changed

The Builder template became a playable ant-colony RTS. Its parts:

- **Core** (`crates/core`): every rule.
  - **Map:** seeded, rotation-symmetric map generation, with tunnels at least Tunnel Width wide.
  - **Simulation:** integer and deterministic, run at 10 ticks per second.
  - **Movement and sight:** flow-field pathfinding (Walk, Dig, Fly) and line-of-sight fog.
  - **Economy:** automatic Harvester Ants with Gather Focus and saturation, and blueprints whose cost Carpenter Ants carry.
  - **Production:** separate training and research queues per queen or hatchery, with housing and the pop cap.
  - **Content:** 12 species, colony and species upgrades, three tiers.
  - **Combat:** flat armor (minimum 1), splash, healing and digging.
  - **Match flow:** events (Windfall, Hill) and victory.
  - **AI:** Larva, Pupa and Imago, running inside the simulation.
  - **Online:** lockstep with a lobby, per-player turns, resync, agreed drops and host migration.
  - **App:** screens, settings, keybinds, action grid, tooltips and HUD.
  - **Drawing:** the renderer's draw lists.
- **Shim** (`crates/shim`): `Game` (the app) and `Bench` (the Benchmark). Every method is one call into the core.
- **Page** (`web/`):
  - `index.html` and `app.js` build the DOM from the core's view model.
  - `render.js` replays draw lists.
  - `online.js` holds the WebRTC transport, with Trystero 0.25.4 vendored and sha256-pinned.
  - `bench.html` is the Benchmark page.
- **Text** (`data/`): every on-screen word (`strings.json`) and the term hints (`glossary.json`), checked against `GLOSSARY.md` by `crates/core/tests/glossary.rs`.
- **Builder rule changes**, all recorded in the table in `BUILDER-SPEC.md`:
  - the CSP's connect-src adds the pinned Nostr relays;
  - `online.js` may name `RTCPeerConnection`;
  - `online-config.js` may hold numbers and `wss://` URLs;
  - `build.sh` verifies and copies `web/vendor/`.
- **R7 is unchanged:** nothing leaves the page's origin until a player presses Host or Join.

## Triage pass (2026-10-11)

All 30 rows of `TRIAGE.md` are done, by three parallel agents (interface, graphics, simulation) and merged here. The biggest changes:
- **Speed:** the simulation runs at 20 ticks per second, so ants move 2× faster; every duration goes through `rules::ticks`.
- **Building:** only Leaves are carried, and Carpenter Ants carry 50. Walls go up to 64 cells, in sections of 8.
- **New features:** Auto-Scout, Map Revealed, Sandbox (with local Pause and Time Scale), spectated all-AI matches, and a simple Online screen (codes only with `?codes`).
- **Graphics:** distinct species silhouettes, and new icons, waypoint flags and hit effects.
- **Interface:** the HUD updates in place, so clicks and hovers are reliable.

The details are in `SECOND-ORDER.md` rows 37–40.

## Deployed (stage 8, 2026-10-11, by the person's decision)

- **Home page:** https://ptonkle.github.io/, with the game at https://ptonkle.github.io/mandible/.
- **Repository:** `ptonkle/ptonkle.github.io`, public, served by GitHub Pages from `main`. Its local copy is `~/slop/ptonkle.github.io`. Only the built game is published; the source stays local.
- **Updating:** `./scripts/pages.sh` rebuilds the game and copies it into that folder. Then commit and push there. The site's commits use author `ptonk <65458712+ptonkle@users.noreply.github.com>`.
- **Privacy:** the published files hold no personal names. `build.sh` maps the home folder to `~` in paths embedded in the wasm.
- **Check:** `BUILDER_ORIGIN=https://ptonkle.github.io/mandible .venv/bin/python tests/browser/check.py` passed in chromium, firefox and webkit, against the live site.

## What passed

`./scripts/check.sh` on 2026-10-11, commit `23c8f2a` (plus this file and MEASUREMENTS.md):

```
== 5/6 WebAssembly import audit (scripts/wasm_imports.py)
WASM IMPORT AUDIT PASSED
== 6/6 browser check (tests/browser/check.py)
BROWSER CHECK PASSED in chromium, firefox, webkit
BUILDER CHECK PASSED
```

`./scripts/check-online.sh`, same commit:

```
ONLINE CHECK PASSED in chromium, firefox, webkit
ONLINE MATCH CHECK PASSED in chromium, firefox, webkit
```

Slow checks (run by hand, results in `MEASUREMENTS.md`):
- **Performance:** 2.8 ms average tick with 1,200 ants.
- **AI ladder:** Imago beat Pupa 10/10 and Pupa beat Larva 10/10.

## What remains

Open rows in `SECOND-ORDER.md`. Those marked *person* are decisions for the person:

- **Row 36, match length.** Now a median of 5:39 (Imago v Pupa) and 4:53 (Pupa v Larva) at 20 ticks per second.
- **Row 40:** the name "Map Revealed".
- **Row 25, Train 10 modifier.** Ctrl+W cannot be caught by a web page, so Alt also gives Train 10.
- **Row 27, the game's name:** Mandible (chosen 2026-10-11).
- **Row 31, Room Code play.** Online by Room Code, through the public relays, is untested: try it by hand. HTTPS hosting is needed across machines. There is no TURN server, so two players both behind strict NATs cannot connect (row 10).
- **Row 13, Gather Focus.** A new player action that the brief did not mention.
- **GLOSSARY.md:** many rows added during the build are marked ◆ (proposed) and need the person's review.
- **Row 33, Benchmark labels.** They are constants in `bench.rs`, not `data/`. Enemy structures seen earlier are not remembered in fog.

Check by hand (stage 7, record in `TRIAGE.md`):
- Play a whole match against each difficulty.
- Try every grid tab, walls (drag), waypoints (several per species; right-click to clear), Retreat, Evolve, and an expansion queen.
- Change the Match Settings and confirm they are kept after a reload.
- Remap a hotkey.
- Open the Benchmark in a normal (non-headless) browser and judge the frame rate.
- Read the tooltips and term hints for wording.

## What the next session must not assume

- Identifiers banned in the core (`time`, `net`, `env`, `io`, `rand`, …) are banned even as field names. The boundary test reads every file in `crates/core/src`.
- The page's JavaScript may hold no number other than 0 and 1. Pass numbers from the core.
- Every new on-screen word needs a glossary row first. `tests/glossary.rs` fails otherwise.
- The AI ladder and performance tests are `#[ignore]`d and are not part of `check.sh`. Rerun them in release after any rules, map or AI change.
- `tool_core` builds with `opt-level = 3` in dev and test (root `Cargo.toml`). The simulation tests depend on it for speed.
