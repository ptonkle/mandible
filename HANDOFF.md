# Handoff

Stage 9, written by the agent. `C5 Consolidate in your own words`.

A fresh session starts from this file, not from memory.

## Start here next session (written 2026-10-12)

**State:** everything is committed on `main` (no worktrees left) and deployed. The live site is https://ptonkle.github.io/ (home page with a random battle picture from `img/mandible/`) and the game is at https://ptonkle.github.io/mandible/. All TRIAGE rows 1–89 are done. Last results: `check.sh` and `check-online.sh` passed in three engines. The AI ladder passes on all three map types (Imago beat Pupa 9–10/10, Pupa beat Larva 10/10). Perf with 1,200 ants: 1.2 ms average tick.

**Questions for the person:**
- **Siege Ant Species Upgrade** (SECOND-ORDER row 55). The Upgrades tab is full, so adding one means rearranging it.
- **Saved Default Match Settings** from before Map Type Random still say Tunnels (row 63). The person picks Random once, or presses Reset. A migration could do this instead.
- **"Everyone on one team" (TRIAGE row 81).** The cause was never reproduced. Start is now refused in three places. If the person remembers the setup, check that path.

**For the person to try by hand:**
- **2v2 lag:** confirm the performance work fixed it (row 72; the next suspect is `ui()` JSON every frame).
- **Sound loudness:** the new levels: every cue at half level, default Volume 50%.
- **Leaves balance:** play against the AI and compare in the end graphs. Balanced is now 7:2:3, and the Imago AI still ends with about 1,200 unspent Leaves because it runs short of Food.
- **Room Code play across real networks** (row 31). There is no TURN server (row 10).
- **Match History for an online match** (row 50).

**Known loose ends:**
- **Ctrl+W / Ctrl+Q** can't be caught (row 25; Alt also gives Train 10).
- **Maze hunting** is only partly fixed (row 42).
- **"Map Revealed"** uses a never-say word (row 40).
- **The Tunnels ladder** was at 8–9/10, close to the bar.
- **`map_generation_time` (ai test)** can fail on Maze when other heavy jobs run at the same time. Rerun it alone.

**How the work was done (keep doing this):**
- Feedback goes into `TRIAGE.md` rows first.
- The rows are split across parallel worktree agents with clear file ownership, then merged on main.
- Then run `check.sh`, `check-online.sh` and the ladder for all three map types, and record the results here.
- SECOND-ORDER rows were renumbered in merge order, so agents' notes in TRIAGE may cite their own pre-merge row numbers.

**Deploy (only when the person says so; each deploy needs their go-ahead):**
1. Run `./scripts/pages.sh`.
2. In `~/slop/ptonkle.github.io`, scan for personal names.
3. Commit as `ptonk <65458712+ptonkle@users.noreply.github.com>` and push.
4. Run `BUILDER_ORIGIN=https://ptonkle.github.io/mandible .venv/bin/python tests/browser/check.py`.

The battle pictures came from a throwaway scan of all-AI Imago matches, which was not kept. Seeds and settings for each picture are in TRIAGE row 66's session notes, if needed: format, seed, biome, map type and clock time, e.g. 2v2 seed 19 Flower Field Maze at 7:51.

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
- **Redeployed 2026-10-11** from commit `3dd622e` (site commit `46d17d5`): lobby, Ready Up, Rematch, sound, volume slider, maps and balance. Name scan clean; live browser check passed in all three engines.
- **Redeployed again 2026-10-11** from commit `ac7cb2a` (site commit `34766d4`): batch 3 (TRIAGE rows 61–89) and the home page battle pictures (12 in `img/mandible/`, one at random). Live browser check passed in all three engines.

## Match History (TRIAGE row 88, 2026-10-11)

A fifth Main Menu leaf opens Match History. It holds the last 50 matches, numbered with no date, newest first; `crates/core/src/history.rs` does the work. `./scripts/check.sh` result on this branch: BUILDER CHECK PASSED, with the BROWSER CHECK PASSED in chromium, firefox and webkit, including the new Match History checks. The details are in `SECOND-ORDER.md` rows 47–51.

## Feedback batch 3 merged (2026-10-11, TRIAGE rows 61–89)

Six parallel agents (balance and species, movement and performance, match setup, in-match flow and sound, Match History, Menu Backgrounds), merged on main at the commit after `5b3e459`.
- `./scripts/check.sh`: BUILDER CHECK PASSED (browser check in chromium, firefox, webkit).
- `./scripts/check-online.sh`: ONLINE CHECK PASSED and ONLINE MATCH CHECK PASSED in three engines.
- AI ladder: Imago beat Pupa 9/10 Tunnels (median 5:24), 10/10 Open Ground (5:37), 10/10 Maze (5:54); Pupa beat Larva 10/10 on all three.
- Perf: 1,200 ants average tick 1.17 ms (was 2.9 ms). `map_generation_time` failed once on Maze (54.9 ms worst) while the browser checks ran alongside; alone it passed twice (43.9 ms worst).
- SECOND-ORDER rows were renumbered 47–72 at merge, in merge order; agent notes in TRIAGE that cite "SECOND-ORDER row 47–53" use the agents' own numbers.
- Open: TRIAGE row 66 (battle screenshots for the home page; the person picks the image).

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
- **Performance:** 2.9 ms average tick with 1,200 ants (latest, commit `3dd622e`).
- **AI ladder (latest):** Imago beat Pupa 9/10 on Tunnels, 10/10 on Open Ground and Maze; Pupa beat Larva 10/10 on every map type. Typical match: Tunnels 5:10, Open Ground 5:19, Maze 6:02.
- **Latest `check.sh` and `check-online.sh`** (commit `b286aef` and the slider): BUILDER CHECK PASSED; ONLINE CHECK PASSED in three engines.
- **Balance and species pass** (TRIAGE rows 63, 67–71, 78, 82, 83, 86; 2026-10-11): `check.sh` BUILDER CHECK PASSED (three engines). AI ladder: Imago beat Pupa 8/10 Tunnels, 10/10 Open Ground, 9/10 Maze; Pupa beat Larva 10/10 on all three.
- **Match setup (TRIAGE rows 61, 79, 80, 81, 87; 2026-10-11):** `cargo test --workspace` passes; `./scripts/check.sh`: BUILDER CHECK PASSED (browser check in chromium, firefox, webkit); `./scripts/check-online.sh`: ONLINE CHECK PASSED and ONLINE MATCH CHECK PASSED in three engines.
- **Menu Background (TRIAGE row 89)**: `./scripts/check.sh` on 2026-10-11: BUILDER CHECK PASSED (chromium, firefox, webkit; the Menu Background is shown on every menu and hidden in a match).
- **Rows 62, 74, 85 (Saturation, 2v2 lag, overlap):** `check.sh` BUILDER CHECK PASSED in three engines; 1,200 ants 1.3–1.7 ms; 2v2 AI match 163 µs average tick; Benchmark tick 2.04 ms in Chromium; ladder on Tunnels Imago 9/10, Pupa 10/10. `check-online.sh` not rerun (the transport is unchanged; `terrain_patch` is local drawing).

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
