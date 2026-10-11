# Mandible

A macro-focused real-time strategy game about ant colonies, played in the browser: 1v1, 2v2 or Free-for-all, against the AI or online. You never select an ant. You train them, place blueprints, set waypoints per species and buy upgrades, all from a keyboard-shaped action grid.

Every game word is defined in [`GLOSSARY.md`](GLOSSARY.md). The design is in [`BRIEF.md`](BRIEF.md) and the build decisions in [`PLAN.md`](PLAN.md).

## How to play

| Goal | How |
| --- | --- |
| Win | Eliminate every enemy colony: no ants, queens or hatcheries left. Walls and brood cells do not keep a colony alive. |
| Gather | Harvester Ants gather by themselves. Choose a **Gather Focus** on the Colony tab. Every node has a **Saturation** of 8. |
| Spend | Every resource goes into one **stockpile**, spendable anywhere at once. |
| Build | Place a **blueprint** (Build tab). Carpenter Ants carry its cost from a queen to the blueprint, then build it. |
| Housing | Queens give 20 and brood cells give 15, up to the **Pop Cap** of 300. Training waits while **Housing Full**. |
| Train | From anywhere, with no selection: Tier 1 species from queens, the rest from their **hatchery**. |
| Fight | Set **waypoints** per species (Waypoints tab). Ants walk there and fight whatever they meet on the way. **All Ants** sets one waypoint for every combat species. |
| Tech | **Evolve** to Tier 2 and Tier 3 (Colony tab) to unlock species and upgrade ranks. |
| Sandbox | **Sandbox** (Match Settings): unlimited resources and Tier 3 for human colonies, plus Pause and Time Scale. |
| Scout | **Auto-Scout** (Colony tab): one combat ant explores by itself. **Map Revealed** (Match Settings) starts every colony with the map Explored. |
| Online | **Host** with the Room Code empty fills in a random code (6 characters, no 1, 0, I or O) to share; **Join** with the code. Codes are not case-sensitive. |
| Events | A **Windfall** (a cache of resources) or a **Hill** (hold it longest to win a bundle) appears every 60–90 s from 1:30. |

### Controls

| Action | Default |
| --- | --- |
| Grid tabs: Ants, Build, Waypoints, Upgrades, Colony | `1` `2` `3` `4` `5` |
| Grid buttons, laid out like the keyboard | `Q W E R` / `A S D F` / `Z X C V` |
| Train 10 / Train Max | Ctrl or Alt + button / Shift + button |
| Place a blueprint | Button, then left-click the map. Shift + click keeps placing. |
| Set a waypoint (replaces that species' waypoints) | Button, then left-click the map |
| Add a waypoint (up to 4 per species) | Button, then Shift + click; stays in waypoint mode |
| All Ants waypoint, from any grid tab | `Tab`, then left-click the map (Shift + click adds) |
| Wall | Button, then press, drag and release |
| Clear a waypoint; cancel a blueprint | Right-click it |
| Cancel a queued item | Click it in the queue bar: one at a time, refunded. Ants of one species show once per queen or hatchery, with a ×N count. An upgrade shows its rank (I, II, III) on its picture; a Colony Upgrade has a gold double border, a Species Upgrade a green one |
| Pan the view | Drag, arrow keys, screen edges (all four, even under the top bar), or the minimap. **Edge Pan Speed** (Player Settings): Very Slow to Very Fast |
| After the match | Graph tabs: Food, Nectar, Leaves gathered, Combat Ants, Population, Killed, Lost; one line per colony, and every colony's summary |
| Zoom | Mouse wheel |
| Home Nest | `Space` |
| Menu (Resume, Concede); cancel placement | `Esc` |
| Pause, Time Scale (×½ to ×4) | Top-bar buttons, in a Sandbox match or while spectating; never online |
| Spectate an all-AI match | Play vs AI: set Red to an AI. The Fog of War button picks whose view to show (Off: the whole map) |

Every hotkey can be remapped in **Player Settings → Keybinds**. Ctrl+W cannot be caught by a web page, so Alt also gives Train 10.

## Build, test and serve

```bash
./scripts/setup.sh          # once: Python venv with Playwright and three browsers
cargo test --workspace      # the core, app, glossary and boundary tests
./scripts/build.sh          # release WebAssembly build into dist/
python3 scripts/serve.py    # http://127.0.0.1:8000/
./scripts/check.sh          # everything, before a deploy
```

Slow checks, run by hand and recorded in `MEASUREMENTS.md`:

```bash
cargo test --release -p tool_core --test perf -- --ignored --nocapture   # 1,200 ants within budget
cargo test --release -p tool_core --test ai -- --ignored --nocapture     # Imago > Pupa > Larva
```

Needs Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` 0.2.127, and Python 3.

## Layout

| Path | Holds |
| --- | --- |
| `crates/core/src/rules.rs` | balance tables: species, structures, upgrades, tiers |
| `crates/core/src/map.rs` | seeded, rotation-symmetric map generation |
| `crates/core/src/world.rs`, `ants.rs` | the simulation: commands, production, economy, combat, events, victory |
| `crates/core/src/nav.rs`, `fog.rs` | flow-field pathfinding; line-of-sight fog of war |
| `crates/core/src/ai.rs` | the AI (Larva, Pupa, Imago), inside the simulation |
| `crates/core/src/lockstep/` | online lockstep: lobby, per-player turns, resync, agreed drops |
| `crates/core/src/app.rs`, `keys.rs` | screens, settings, keybinds, action grid, tooltips, HUD |
| `crates/shim` | the `wasm-bindgen` methods the page calls; each is one call into the core |
| `web/` | the static page: DOM from the core's view model, canvas drawing, input |
| `data/strings.json`, `data/glossary.json` | every on-screen word, and the term hints |
| `tests/browser/` | the three-engine browser check and the game's interactions |
