# Glossary

The single source of the game's vocabulary. Every name in the code, the on-screen text (`data/`), the docs and conversation comes from this file.

**Status: approved by the person on 2026-10-10.** A term changes only by editing this file first. New terms are added as proposed (◆) until the person sees them.

How to read the tables:

- **Term** is the exact spelling and capitalisation for on-screen text.
- **Origin**: ★ means the term comes from the person's request; ◆ means the agent proposed it.
- **Never say** lists the words that must not be used for the same idea, because they would blur it with something else.

## Rules for using these words

1. **One idea, one word.** Do not use synonyms for variety. If the text says "nest" in one place, it never says "base" in another.
2. **Never reuse a word.** A word means one thing only. "Wall" is a player-built wall and nothing else; map terrain is "rock" or "dirt".
3. **Capitals.** On-screen, game terms are capitalised when they name a specific thing: species (Fire Ant), hatcheries (Acid Ant Hatchery), resources (Food, Nectar, Leaves), actions (Train 10, Retreat), tiers (Tier 2) and stats (Armor). Generic nouns stay lower-case in sentences: "your ants", "a queen", "a wall".
4. **Numbers.** Tiers use digits (Tier 1, 2, 3). Upgrade ranks use roman numerals (Armor I, II, III), so the two cannot be confused.
5. **Resource amounts.** Show an amount as icon + number, and a rate as number + "/s" ("4 Food/s"). Rates are always per second. In prose the resource names are mass nouns: "costs 50 Leaves", "costs 25 Food". There is no singular "Leaf" for the resource.
6. **Code.** Identifiers in code use the same words, in snake_case or CamelCase: `Species::FireAnt`, `pop_cap`, `Blueprint`. A code name for something in this glossary that is not listed here is a bug.
7. **New ideas.** A new concept gets a row here before it gets a name in code or on screen. The glossary is kept up to date as the game grows; it is never written once and left behind.
8. **No prose on screen.** On-screen text is labels and label–value pairs, not sentences. Use prose in fewer than 1% of cases, and only where a list cannot work (for example a confirmation question). Example tooltip for a species button:

   ```
   Turtle Ant        Tank · Tier 2
   Cost              60 Food · 20 Nectar
   Pop Cost          3
   Health            40
   Armor             12
   Damage            4
   Move Speed        Slow
   Requires          Turtle Ant Hatchery
   ```

   Never write: "This is an armored ant. It has 40 health and 12 armor."
9. **Term hints.** Any glossary term that is not obvious (for example Armor, Pop Cost, Saturated, Tier) can be hovered wherever it appears on screen, including inside a tooltip, to show a **term hint**. A term hint is the one allowed exception to rule 8: a single short sentence, with no more than one clause where possible.

   ```
   Armor   Each point of Armor reduces damage taken by 1, to a minimum of 1.
   ```

   Every term that gets a hint has its hint text written in `data/` and checked against its meaning here. A term hint uses only glossary words.

## 1. Match and players

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Match | ◆ | One game, from the start until a team wins. | round, session; "game" is fine casually but not in UI text |
| Player | ★ | A person or an AI taking part in a match. | user (in game text) |
| AI | ★ | A player controlled by the computer. "AI player" when the context needs it. | bot, CPU, computer |
| Difficulty | ★ | How capable an AI player is: Larva, Pupa or Imago. The names describe the AI's skill, never how hard it is to face, so they mean the same whether the AI is an ally or an enemy. | level, skill, AI level |
| Larva | ★ | The weakest difficulty. Term hint: "Larva: the least capable AI." | Easy, Beginner |
| Pupa | ★ | The middle difficulty. Term hint: "Pupa: a moderately capable AI." | Moderate, Medium, Normal |
| Imago | ★ | The strongest difficulty; the word means a fully grown adult insect. Wherever difficulties are listed, they appear in order Larva → Pupa → Imago, so the order is visible. Term hint: "Imago: the most capable AI." | Hard, Expert, Insane |
| Colony | ◆ | Everything one player owns: ants, queens, walls, blueprints, stockpile, upgrades. | faction, civ, army, empire |
| Team | ◆ | The colonies that win or lose together. In 1v1 and free-for-all, each colony is its own team. | side, alliance |
| Ally | ◆ | Another colony on your team. | friend, teammate (in UI text) |
| Enemy | ◆ | Any colony not on your team. | opponent (in UI text), foe |
| Player color | ★ | The single color that marks everything a colony owns. | team color |
| Format | ◆ | Who plays against whom: 1v1, 2v2 or Free-for-all. | mode (reserved for menu choices) |
| 1v1, 2v2 | ★ | Formats, written exactly this way. | 1 vs 1, one-on-one |
| Free-for-all | ◆ | Four colonies, each its own team (the request's "1v1v1v1"). | FFA (in UI text), 1v1v1v1 (in UI text) |
| Victory / Defeat | ◆ | The end state of a match for each team. A colony is defeated when it has nothing left that is an ant or can make ants: all its ants and queens have died and all its hatcheries are destroyed. It is also defeated when it concedes. Walls and brood cells do not keep a colony alive. | win/lose screens with other wording, "game over" |
| Match Length, Trained, Killed, Lost, Gathered | ◆ | End-screen rows: how long the match lasted, and how many ants the colony trained, killed and lost, and what it gathered. | score, kills, deaths, K/D |
| Waiting On, Resyncing, Player Left | ◆ | Online status flags: a player's turns are late; the match is re-copying from the coordinator; a player left and the AI took over. | lag, desync (in UI text), disconnected |
| Concede | ★ | To give up the match on purpose. The colony is defeated at once. | surrender, quit, forfeit, GG |
| Eliminated | ◆ | A colony that has been defeated while the match goes on (2v2, Free-for-all). | dead, knocked out, out |
| Lobby | ◆ | The online room before a match, where players join and match settings are chosen. | room, party, queue |
| Host | ◆ | The player who created the lobby. | owner, admin |
| Room Code | ◆ | The name two or more players type to meet online. | lobby code, room id, server |
| Invite Code, Reply Code | ◆ | The two codes of the pasted-code path: the host gives an Invite Code, the joiner answers with a Reply Code. No relay is used. | offer, answer, SDP, token |
| Join, Connect, Leave | ◆ | Online actions: Join a room, Connect after pasting codes, Leave a lobby. | enter, link up, quit (in the lobby) |
| Seat | ◆ | One place in a match, shown by its player color. | slot, spot, position |
| You | ◆ | The local player's seat. | me, player 1 |
| Open, Closed | ◆ | Lobby seat states: Open waits for a player; Closed is not played. An Open seat at Start is filled by a Pupa AI. | empty, locked (that is the tier gate) |
| Blue, Orange, Green, Pink | ◆ | The player colors of seats 1–4, chosen to be easy to tell apart. | red, purple, team color |

## 2. Map

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Map | ★ | The whole playing area of a match. | level, arena, board, world |
| Seed | ◆ | The number that fully determines a generated map. The same seed gives the same map. | map code, map ID |
| Biome | ◆ | The visual setting of a map, such as Picnic or Flower Field. It changes art only, never rules. | theme, setting (reserved for settings), environment |
| Picnic, Flower Field | ★ | The first two biomes. | |
| Tunnel | ★ | A long, narrow passage that connects chambers. | corridor, path, lane, passage |
| Chamber | ★ | An open room in the map. | room, cave, cavern, area |
| Width | ◆ | Tunnel width, measured in ant-widths. The minimum is about 10. | size (for tunnels) |
| Rock | ★ | The map's solid ground between tunnels and chambers. It cannot be destroyed. | wall, earth, stone, terrain wall, map wall |
| Dirt | ★ | A stretch of map ground that ants can dig through. It has health and usually hides a cache. Drawn clearly differently from rock. | soft earth, destructible wall, breakable wall, soil, mud |
| Dig | ◆ | To break through dirt, by attacking it. | mine, break, destroy (for dirt) |
| Home Chamber | ★ | The chamber where a colony begins, with nodes for all three resources nearby. | start chamber, spawn, home base, start location |
| Nest | ★ | A chamber a colony has settled with at least one queen. The first one is the Home Nest. At most about 5 per colony. | base, hive, town, expansion, outpost |
| Home Nest | ◆ | A colony's first nest, in its home chamber. | main, main base |

## 3. Resources and economy

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Resource | ★ | One of Food, Nectar, Leaves. | material, currency, goods |
| Food | ★ | The basic resource, used for almost everything. | crumbs (as a resource name), sugar |
| Nectar | ★ | The resource for advanced and enhanced ants and upgrades. | honey, honeydew, sugar |
| Leaves | ★ | The resource for building: queens and walls. | wood, leaf (as a resource name), material |
| Stockpile | ◆ | A colony's shared pool of delivered resources, spendable anywhere at once. | bank, storage, pool, stash, treasury |
| Node | ★ | A place on the map where a resource can be gathered. Specific kinds are below. | resource field, deposit, patch, source |
| Crumb Pile | ◆ | A Food node. | food node (in UI text) |
| Flower | ◆ | A Nectar node. | nectar node (in UI text), blossom |
| Leaf Litter | ◆ | A Leaves node. | leaf pile, leaf node (in UI text) |
| Depleted | ★ | A node with nothing left to gather. It disappears or greys out. | exhausted, empty, mined out, dry |
| Cache | ★ | A rich, one-off resource deposit behind dirt. | stash, treasure, hoard |
| Windfall | ◆ | A cache that appears during a match as an event. | drop, airdrop, bonus |
| Gather | ★ | To fill an ant's inventory from a node. | harvest, mine, collect, farm |
| Inventory | ★ | The resources an ant is carrying right now. | load, cargo, bundle, haul |
| Carry Capacity | ◆ | The most a species' inventory can hold. | carry, capacity (alone), cargo size |
| Laden | ◆ | An ant whose inventory is not empty. Laden ants can be slower. | loaded, burdened, encumbered |
| Deliver | ★ | To empty an ant's inventory: a gatherer at a queen (into the stockpile), a builder at a blueprint. | drop, return, deposit, bring back |
| Drop-off | ★ | The place where gatherers deliver resources into the stockpile. Only queens are drop-offs. | collection point, depot, storehouse, warehouse |
| Gather Rate | ★ | How fast a species gathers, per ant. | harvest speed, income (that is the colony total) |
| Income | ★ | A colony's estimated resources per second, for each resource. Shown in the resource bar as "4 Food/s". Calculated by the core, never by the page. | gather rate (that is per ant), eco, per minute |
| Saturated | ★ | A node with as many gatherers as it can usefully hold. | full, maxed, oversaturated (unless meant exactly) |

## 4. Ants

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Ant | ★ | Any mobile creature a colony trains. In rules text "ants" does **not** include queens; say "ants and queens". | unit, creature, minion, troop, bug |
| Species | ◆ | A kind of ant, such as Fire Ant. Waypoints, upgrades and training all work per species. | unit type, class, caste, breed, type |
| Role | ◆ | A short tag for what a species does: Gatherer, Builder, Melee, Ranged, Tank, Support, Flyer, Brute, Bomber. Shown in tooltips. | job, archetype, class |
| Gatherer | ★ | Role: gathers and delivers resources by itself. | worker, drone, peon, villager, harvester (the role) |
| Builder | ★ | Role: carries resources to blueprints and builds them, by itself. | worker, engineer, constructor |
| Melee | ★ | Role: fights at contact range. | close combat, brawler |
| Ranged | ★ | Role: fights from a distance. | shooter, archer, artillery |
| Tank | ★ | Role: soaks damage with high health or armor. | tanky (as a noun), wall (never) |
| Support | ◆ | Role: helps other ants, for example by healing. | healer (the role), medic |
| Flyer | ★ | Role: flies over walls. Rock and dirt still block it. | air unit, flier |
| Brute | ◆ | Role: heavy melee whose every hit deals Splash damage. | cleaver, ultralisk, siege |
| Bomber | ◆ | Role: dies on purpose to deal area damage. | suicide unit, kamikaze |
| Population | ★ | The total pop cost of all of a colony's ants. Shown against housing as "Population 120 / 150". | supply, food (that is a resource), pop count |
| Housing | ★ | How much population a colony can support right now, provided by its queens and brood cells. It never exceeds the pop cap. | supply, pop limit, current cap, capacity |
| Housing Full | ◆ | Population has reached housing, so training waits until more housing is built. | supply blocked, pop blocked, capped |
| Pop Cap | ★ | The hard maximum for housing: 300. Building more brood cells cannot go past it. | supply cap, unit limit, max pop |
| Pop Cost | ★ | How much population one ant of a species uses. Stronger species cost more. | supply cost, size, weight |

### Species (proposed names, tiers and roles)

**Naming test:** a player who has never heard of the real ant must still be able to guess its role from the name alone. Real names are used only when they pass this test. Tier placement is a starting suggestion for the plan.

| Species | Origin | Role | Suggested tier | Why the name passes the test |
| --- | --- | --- | --- | --- |
| Harvester Ant | ◆ | Gatherer | 1 | "Harvester" says gathering. It is also a real ant. It carries more, and slows down less when laden. |
| Soldier Ant | ◆ | Melee | 1 | "Soldier" says fighter. It is the basic melee ant. |
| Carpenter Ant | ★ | Builder | 1 | "Carpenter" says builder. It is also a real ant. |
| Fire Ant | ★ | Melee | 2 | A weak, fast ant. The name is the person's choice, kept knowing that players may first expect burning damage. |
| Turtle Ant | ★ | Tank | 2 | "Turtle" says shell and toughness. It is also a real ant. |
| Acid Ant | ★ | Ranged | 2 | "Acid" says it sprays something. It replaces Wood Ant, which players would not link to acid. It is not a strict common name, but the request itself says "acid ants". |
| Nurse Ant | ◆ | Support | 2 | "Nurse" says healer. |
| Flying Ant | ★ | Flyer | 3 | Says exactly what it does. |
| Exploding Ant | ★ | Bomber | 3 | Says exactly what it does. |
| Trap-jaw Ant | ◆ | Melee | 3 | Heavy-hitting elite melee. "Trap-jaw" says a big bite. It replaces Bullet Ant, which players would read as ranged. |
| Dinosaur Ant | ★ | Brute | 3 | Huge, slow brute whose every hit deals Splash damage (like StarCraft II's Ultralisk). "Dinosaur" says huge brute. It is also a real ant. |
| Leafcutter Ant | ◆ | Gatherer | 2 | Optional. "Leafcutter" says it gathers Leaves, so it fits a Leaves specialist only, never a general gatherer. |
| Army Ant | ◆ | Melee | — | Reserved for a future civilization's swarm unit. "Army" says many fighters. |

Rejected names. Do not reuse them without passing the naming test.

| Name | Why it was rejected |
| --- | --- |
| Wood Ant | Players would expect a builder or wood gatherer, not an acid sprayer. |
| Bullet Ant | Players would expect a ranged shooter, not a melee ant. |
| Honeypot Ant | Proposed as a mobile drop-off, which contradicts the decision that only queens are drop-offs. |
| Bulldog Ant | Reads as melee, but is too close in meaning to Soldier Ant and Trap-jaw Ant. |

## 5. Queens and building

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Queen | ★ | A stationary colony member that anchors a nest. Queens are the only drop-offs and the only way to train Tier 1 species. There is one kind of queen. | building, base, Brood Queen |
| Hatchery | ◆ | A built structure that lets a colony train one species above Tier 1, named "‹Species› Hatchery" (for example Acid Ant Hatchery). It has health and shows the player color. The person suggested "chamber", but that word already means a map room, and "nursery" is too close to Nurse Ant. | chamber (that is a map room), building, barracks, den |
| Brood Cell | ◆ | A built structure that adds housing. It has health and shows the player color. | house, supply depot, chamber (that is a map room), den, pod |
| Structure | ◆ | Umbrella word for anything built from a blueprint: queens, hatcheries, brood cells, walls. Use it only when all four are meant. | building |
| Wall | ★ | A barrier built by a player. It can be destroyed and shows its colony's color. | barricade, fence, barrier; never used for rock or dirt |
| Blueprint | ◆ | A placed outline of a structure, waiting for resources and builders. It becomes the real thing when complete. | plan, ghost, foundation, construction site, queued building |
| Place | ★ | To put a blueprint on the map. | build (that is the Carpenter Ants' job), drop, queue (on the map) |
| Build | ★ | What Builder ants do at a blueprint. | construct, raise, make |
| Complete | ◆ | A blueprint that has every resource and all its build work. It turns into the structure. | finish, done, built |
| Destroyed | ★ | A wall, hatchery, brood cell or blueprint reduced to zero health. | dead, broken, razed |
| Dies / Killed | ◆ | An ant or queen reduced to zero health. | destroyed (for living things), defeated |

## 6. Progression

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Tier | ★ | A colony's stage of advancement: Tier 1, Tier 2, Tier 3. Higher tiers unlock species and upgrades. | age, era, level, tech level, generation |
| Evolve | ★ | To pay resources to reach the next tier. The action is "Evolve to Tier 2". | advance, age up, tech up, level up |
| Evolution | ★ | An evolve in progress, or its cost: "Evolution: 60s". | upgrade (never for tiers) |
| Upgrade | ★ | A permanent, colony-wide improvement that applies to every ant of a species, or to every ant (a colony upgrade). Ranked I, II, III. | research, tech, enhancement, buff |
| Species Upgrade | ◆ | An upgrade that affects one species. | unit upgrade |
| Colony Upgrade | ★ | An upgrade that affects every species. | global upgrade, universal upgrade |
| Rank | ◆ | An upgrade's level: Armor I, Armor II, Armor III. | level, tier (that is the colony stage), stage |
| Unlock | ◆ | To become available because of a tier. | enable, open |
| Locked | ◆ | Not yet available. The tooltip shows "Requires Tier 2". | disabled (that is the match setting), greyed |
| Disabled | ★ | Turned off in the match settings, for the whole match. | locked (that is the tier gate), banned |
| Ready, Queued, Max Rank, Active | ◆ | Action button states: Ready to use; Queued (already in the queue); Max Rank (no rank left); Active (the current mode or setting). | available, pending, maxed, selected |
| Upgrade Complete | ◆ | Notice when an upgrade's rank is reached. | researched, done |

## 7. Stats and combat

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Health | ★ | Damage an ant, queen, wall or blueprint can take before it dies or is destroyed. | HP (in UI text), hit points, life |
| Cost | ★ | What something takes from the stockpile, as icon + number per resource. | price |
| None | ◆ | A cost or value of nothing. | free, n/a, — |
| Damage | ★ | What one attack deals before armor. | attack (as a stat), power, strength, DPS |
| Armor | ★ | A flat number subtracted from every hit taken: damage taken = Damage − Armor. | defense, shield, resistance, protection |
| Range | ★ | How far an ant can attack, in ant-lengths. Melee range is contact. | reach, distance |
| Move Speed | ◆ | How fast an ant moves. Always written in full. | speed (alone), velocity, pace |
| Attack Speed | ◆ | How often an ant attacks. Always written in full. | speed (alone), fire rate, cooldown |
| Vision | ★ | How far an ant or queen sees, with line of sight. | sight range, view |
| Heal Rate | ◆ | Health a Support species restores per second. | regen, healing power |
| Build Rate | ◆ | How fast a Builder turns delivered resources into progress. | build speed, construction speed |
| Attack | ★ | The verb: one ant hitting a target. | (never a stat name) |
| Target | ◆ | What an ant is attacking. Chosen automatically. | focus, aggro |
| Splash | ◆ | Damage dealt to every enemy in an area, as with Exploding Ants and Dinosaur Ants. | AoE (in UI text), blast |
| Duration | ★ | How long training, research or evolving takes, in seconds. | time, build time, cooldown |
| Effect | ◆ | What an upgrade or structure changes, as a label–value row. | bonus, buff |
| Next | ◆ | The tier an Evolve reaches. | upcoming |
| Status | ◆ | A short state label in a tooltip or the lobby. | info, state |
| Very Slow, Slow, Normal, Fast, Very Fast | ◆ | Move Speed shown as a word in tooltips. | numbers in tooltips |
| cells | ◆ | The unit for Range, Vision and Splash: one cell is about one ant wide. | tiles, squares, meters |

## 8. Player actions

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Action | ★ | Anything the player can do from the action grid: train, place, upgrade, evolve, retreat. | ability, command, order, spell |
| Waypoint | ★ | A map point where every ant of one species goes. A species can have several. | rally point, flag, marker, target, move order |
| All Ants | ★ | The Waypoints-tab button that sets one waypoint for every combat species at once, replacing theirs. | everyone, all units, army |
| Set Waypoint | ★ | Place a waypoint for a species. | order, command, send |
| Clear Waypoint | ★ | Remove one waypoint (right-click it). | delete, cancel, remove |
| Split | ★ | How a species shares itself evenly between its waypoints. | spread, divide |
| Train | ★ | Pay for and produce one ant, from anywhere, without visiting a queen. | build (that is for blueprints), make, spawn, hatch, recruit |
| Train 10 | ★ | Ctrl + Train: queue ten at once. | batch, train ×10 |
| Train Max | ★ | Shift + Train: queue as many as the stockpile can pay for. | train all, spend all |
| Queue | ★ | A colony's list of ants, upgrades and evolutions waiting or in progress. | build order, production list |
| Queue Bar | ★ | The row of icons beside the resource counts that shows the queue. | production bar, build queue |
| Cancel | ◆ | Remove an item from the queue and get its resources back. | delete, dequeue |
| Retreat | ★ | A toggle. While on, every ant except Gatherers and Builders pulls back towards its colony's nests. | recall, fall back, return, regroup |
| Gather Focus | ◆ | Which resource Harvester Ants favour: Balanced (Food 2 : Nectar 1 : Leaves 1), or Food, Nectar or Leaves (3 : 1 : 1 toward it). | priority, eco setting, worker split |
| Balanced | ◆ | The default Gather Focus. | even, default |
| Placement Blocked | ◆ | Notice: the blueprint cannot go there (rock, another structure, a node, not explored, or too far from a queen). | invalid placement, can't build here |
| Not Enough | ◆ | Notice prefix for a missing resource: "Not Enough Nectar". | insufficient, need more |
| Requires | ◆ | Label for what an action still needs: a tier, a hatchery or a queen. | needs, prerequisite |
| Select | — | **There is no selection in this game.** Never use this word for gameplay. | |

## 9. Interface

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Action Grid | ★ | The keyboard-shaped grid of action buttons at the bottom left. | command card, hotbar, toolbar, ability bar |
| Action Button | ◆ | One cell of the action grid. | slot, icon, tile |
| Grid Tab | ◆ | One page of the action grid, if there are more actions than buttons (for example Ants, Queens, Upgrades). *Still open.* | page, menu, category |
| Ants, Build, Waypoints, Upgrades, Colony | ◆ | The five grid tabs, on keys 1–5. | units, buildings, research, economy |
| Pan, Edge Pan | ◆ | Moving the view: Pan with the arrow keys or by dragging; Edge Pan moves it when the pointer touches the screen edge. | scroll, camera move |
| Benchmark | ★ | The Player Settings button that runs the performance-test scenario live, with its budget. | perf test, stress test |
| Hotkey | ★ | The key that triggers an action button. It follows the button's position on the keyboard. | shortcut, keybind (that is the setting), key |
| Keybinds | ★ | The Player Settings screen where hotkeys are remapped. | controls, key mapping |
| Resource Bar | ◆ | The top display of Food, Nectar and Leaves, each with its income, plus population. | HUD, resource panel |
| Minimap | ◆ | The small overview of the map, if added. | radar, map (alone) |
| Health Pie | ◆ | How a structure shows its health: its player-color fill shrinks like a pie chart as health falls. Structures only; ants show no health. There are no health bars. | health bar, HP bar, life bar |
| Tooltip | ◆ | The hover box for a button: label–value pairs only (name, role, tier, cost, stats, requirements). No sentences. | hint, info popup |
| Term Hint | ★ | The short definition shown when hovering a glossary term on screen. One sentence. | tooltip (that is for buttons), definition popup, help text |
| Announcement | ◆ | A short label on screen, such as "Event: Windfall". No sentences. | alert, notification, popup |

## 10. Vision

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Fog of War | ★ | Everything a colony cannot currently see. | shroud, darkness, fog (alone, in UI text) |
| Unexplored | ◆ | Map never seen. Drawn black. | hidden, unknown, black |
| Explored | ◆ | Map seen before but not visible now. Shows rock, dirt and the last known state. | fogged, remembered |
| Visible | ◆ | Map currently in some ant's or queen's line of sight. | revealed, lit, seen |
| Line of Sight | ★ | Vision that stops at rock, dirt and walls, so it does not see around corners. | LOS (in UI text), view cone |

## 11. Events

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Event | ★ | Something the match itself does at a random time, announced to every player. | random event (in UI text), happening, incident |
| Windfall | ◆ | Event: a cache appears somewhere on the map. | resource drop, airdrop, bonus |
| Hill | ◆ | Event: a zone that rewards the colony holding it. "Hold the Hill." | king of the hill, capture point, objective, KOTH |
| Holding | ◆ | Having ants in the hill with no enemy ants there. | capturing, controlling, owning |
| Contested | ◆ | The hill has ants from more than one team in it. | disputed, blocked |

## 12. Menus and settings

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Main Menu | ★ | The first screen. | title screen, home, start menu |
| Menu | ◆ | The in-match menu (Esc): Resume, Concede. | pause menu, options |
| Resume | ◆ | Close the Menu and carry on. | continue, unpause |
| Map Size, Chamber Size | ★ | Match Settings for the size of the map and of its chambers: Small, Normal or Large. | map scale, room size |
| Hill Won, Hill Ended | ◆ | Announcements when a Hill closes: won by a team, or ended with no single holder. | captured, tied |
| Play vs AI | ★ | Main-menu choice: a local match against AI players. | skirmish, single player, practice |
| Online Multiplayer | ★ | Main-menu choice: create or join a lobby. | online, PvP, versus |
| Match Settings | ★ | The options that shape a match: chamber size, tunnel width, resource density, disabled species, game speed, starting resources, biome, format, and so on. | game settings, options, rules, config |
| Default Match Settings | ★ | The match settings saved from the main menu, which a new lobby or Play vs AI match starts from. | presets, defaults (alone) |
| Player Settings | ★ | Personal options that never affect a match: keybinds, volume, and so on. | options, preferences, settings (alone) |
| Game Speed | ★ | A match setting that scales how fast time runs. | tick rate, speed (alone) |
| Small, Normal, Large / Low, Normal, High | ◆ | Values of Map Size and Chamber Size (Small, Normal, Large) and of Resource Density and Starting Resources (Low, Normal, High). | tiny, huge, rich, poor |
| On, Off | ◆ | Values of two-way settings such as Events and Edge Pan. | yes, no, enabled |
| Random | ◆ | A Biome or Seed chosen by the game. | auto |
| Events | ★ | The Match Setting that turns Windfalls and Hills on or off. | random events |
| Resource Density | ★ | A match setting for how much each node holds and how many nodes there are. | richness, abundance |
| Starting Resources | ★ | A match setting: the stockpile each colony begins with. | starting bank, initial resources |
| Campaign | ★ | Story mode. Later, not in the first version. | story, missions |
| Civilization | ★ | Later: a colony type with unique species, bonuses and upgrades. The name is still to be chosen; "Civilization" is a placeholder. | faction, race |

## 13. Internal terms

Used in code, docs and conversation only. **Never shown on screen**, and no term hint.

| Term | Origin | Meaning | Never say |
| --- | --- | --- | --- |
| Overlap | ★ | How far ants may push into each other. Kept small so fights stay readable. | stacking, clumping |
