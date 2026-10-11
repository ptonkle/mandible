# Brief

Stage 1, written by the person. `C1 Frame the problem and set the rules`, `B1 Establish context`.

## The request, word for word

Paste the original request here unchanged. A paraphrase is already an interpretation.

> The idea is a game that's an RTS game where you manage a nest of ants. It is a top down view of a map of tunnels which should be able to fit many ants wide in them like probably at least 10 for the thinnest of tunnels. Performance is going to be important since there will be a lot of tiny little pathfinding ants. I think it would be good if it could handle 300 ants per player. It is a pvp game. 1v1 is a must, and it should be able ot be extended to handle 1v1v1v1 and 2v2. The maps should be able to be quickly randomly generated and need to be generally balanced, so the players do not feel that it's unfair. This should be able to be played online multiplayer. I want the player to have to do very little micro though since I'm not good at that when I play RTS and find it tedious, it should primarily be based on macro. There should be different kinds of ants, ones that can build walls, ones that are ranged, ones that are melee, tanky ones, ones that are good at bringing resources back to base (maybe they can carry more or suffer less of a movement penalty when carrying things), etc.. 3 different types of resources would be good, let's call them food, nectar, and leaves. Food is the most basic and used for everything nectar is for making more complicated/enhanced units, and leaves are for building things. The graphics should evoke that you're fighting over classic ant locations like a picnic could be funny,  or like a field of flowers. The graphics can be simple though. This should be able to be fully controlled with the mouse but there should be hotkeys for all the buildings or other actions. The other actions should be in a grid in the bottom left. For example, if there were 12 actions, arrange them in a 3 row x 4 column, the bottom left button would be mapped to z, the lower right button would be mapped to v, the 2nd row 3rd column would be mapped to d, so the idea is that they mirror the shape of the keyboard. We don't necsssarily need 2 actions. Also if you do a grid this way visually it should have the kinda skewed nature of a keyboard. There should also be dynamic random events on the map like a new drop of resources or a king of the hill zone to keep the players on their toes. There should be a fog of war system too, and it would be good if it actually calculated like vision around corners, so you don't just get a full circle of vision. Resources just fill into a general pool, once they are dropped into a colleciton point they are usable anywhere instantly. Actually building they should have bring the resources to the place to biuld the thing, this will make it harder to cheese building walls instantly.  The map should be a series of tunnels and chambers of various sizes. There should be a 300 ant limit per person, and more sophisticated ants should take up more population space. You cannot destroy the map walls but you can destroy walls that the players make. In this way the walls need to be distinct. Players should be identified by color and all of their different things (buildings, ants, walls) should be identifiable as theirs with with taht color. Make sure the colors are easy to differentiate as possible. Differnt shapes for different things too. The ants can overlap a bit as they would in real life but not too much to make it hard to tell what's going on.
Major inspirations include age of empires 2, starcraft 2.  
Players primary actions in the game are 1. setting waypoints that the ants should go to 2. choosing where buildings should go 3. training units (one thing about this is that I don't want them to have to go to the buildings to train things, they should be able to access all actions globally. There really isn't any selecting of specific ants or buildings or anything), 4. upgrading units (this is global, you do not upgarde them individually), 
Caches of resources behind destructable map walls woul dbe good too, so I guess some map walls should be destrcutable.
ALl the varieties of ants should make sense, so like fire ants could be really fast, turtle ants are tanky. the ants that spray acid. carptenter ants build walls. flying ants can go over walls. maybe a healer ants, other ideas welcome, a specific ant for gathering resources. exploding ants. Use common names for all of these btw that people can recognize and relate with. 
Oh instead of buildings they should actually be different kinds of queen ants. 
Oh I would like there to be a huge amount of customizable options too, so for a game should be able to like turn up and down room size, tunnel size, resource density, disable units, game speed, starting resources, etc.
Upgrades would be things like range, damage, health, armor. Armor scaling should be flat, so like an ant with 12 attack attaching one with 8 armor should do 4 damanage, do not do it with like a percentage dmage reductoin system. Not every type of ant needs all of these upgrades though. You can make upgardes that affect all ants too.
Games should not go longer than 5 min designwise but there does not need ot be a game timer. 
When you load up the game there are generally these options: 1. play against AI 2. play online multiplayer (which should also be able to support AI players, for example if you wanted to play with a friend against 2 AI players), setting to set up default game settings I mentioned before. You can also do personal player settings like remapping keys. Eventually a campaign would be good too but we can skip for the first version.
In the mlutiplayer mode youy should eb able to set up the same setings at the settings default. 
The menu should be like ants themed too, so maybe each of the buttons is like a leaf and when you choose it an ant like pulls it to the side or something.
I guess the way you control your ants is by setting waypoints for each type of ant, so you say "all acid ants go here". You can clear a waypoint by right clicking it. The resource ants should not have to be manually controlled, they should automatically saturate their resource nodes. Resrouce nodes eventually should run out to prevent stalemates. If you want to send ants to multiple locations you can set multiple waypoints and they generally split themselves evenly. Building ants also do not get waypointed they just automaticlaly go to bulid the things that are queued on the map. I think in the longest and largest games you would have like 5 bases, that should be the extreme. There should also be general "retreat" togggle button which pulls your ants back towards your bases.
You should not be able to build all units instantly, there should be 3 tiers (ages in age of empires) that unlock better upgrades and ants. You need to pay a certain amount of resources to get to the higher tier (i guess we could call it evolution). Also there should be upgrades to improve gather rate of things. You should be able to see what is being built in a series of icons near where it displays your resource counts. Similar to AOE2 again. You can built 10 of a unit at a time by holding ctrl when clicking the button or the hotkey, and spend all of your resources to make the unit by holding shift while doing it.   
Players should always have an easy source of all three resources near where they spawn. They spawn with a queen to make the basic ants, like a melee one and then a gatherer one and pehaps one more basic one. 
Another nice thing would be the ability to have similar to choosable civilizations in from AOE2 where they get like unique units, boons, and upgrades. Let's not tackle that for now though.



## The request, by theme

The same request, sorted by theme. Every point comes from the request above. Words in **bold** are terms from `GLOSSARY.md`, and the glossary decides what each one means. Anything in *[brackets and italics]* is the agent's reading and needs the person to confirm it. Answers to the questions the request left open are in "Decisions after the request", at the end of this section.

### 1. Genre and inspirations

- A real-time strategy (RTS) game in which each player manages an ant **colony**.
- Inspired by Age of Empires II and StarCraft II.
- Built around macro rather than micro. The player should do very little micro-management, which the person finds tedious.

### 2. Players, formats and multiplayer

- Player-versus-player. **1v1** is required. The design must extend to **free-for-all** (1v1v1v1) and **2v2**.
- Online multiplayer is required.
- **AI players** can take part in online **matches**, for example two friends against two AIs.
- AI players have three **difficulties**, named for ant life stages so they describe the AI's skill whether it is an ally or an enemy: **Larva** (weakest), **Pupa**, **Imago** (strongest). *[Added 2026-10-10.]*
- Each **player** is identified by a **player color**, and everything a colony owns (**ants**, **queens**, **walls**) shows that color. The colors must be as easy as possible to tell apart.

### 3. Match pacing

- **Matches** should be designed to last no longer than 5 minutes. There is no match timer.
- **Nodes** eventually **deplete**, so that matches cannot stall.
- The longest and largest matches top out at about 5 **nests** per colony.

### 4. Map design

- A top-down **map** made of **tunnels** and **chambers** of various sizes.
- The narrowest tunnel is at least about 10 ants wide.
- Maps are randomly generated, generation is quick, and the result is broadly balanced so that no player feels it is unfair.
- The map's own walls are **rock**, which cannot be destroyed, except for some breakable stretches of **dirt** that hide **caches** of resources.
- Rock, dirt and player-built **walls** must each look clearly different.
- Each player's **home chamber** has **nodes** for all three **resources** close by.
- The art evokes classic ant locations, such as a picnic or a field of flowers (**biomes**).

### 5. Resources and economy

- There are three resources:
  - **Food**: the most basic, used for everything.
  - **Nectar**: used for more complex or enhanced ants.
  - **Leaves**: used for building.
- **Gatherers** fill their **nodes** automatically up to **saturation**, without the player directing them.
- A resource joins one shared pool (the **stockpile**) once it is **delivered** to a **drop-off**. From then it can be spent anywhere, at once.
- Building is different: the resources must be carried to the **blueprint** itself. This stops players from placing walls instantly.
- Some **upgrades** raise **gather rate**.
- Nodes eventually deplete.

### 6. Ants

- There are many **species**, each with a recognisable common name that fits what it does:
  - a species that builds walls (**Carpenter Ant**);
  - ranged ants, including one that sprays acid (**Acid Ant**);
  - melee ants;
  - a tanky species (**Turtle Ant**);
  - a weak, fast species (**Fire Ant**);
  - a species that flies over walls (**Flying Ant**);
  - a dedicated gatherer that is good at bringing resources home, perhaps carrying more or slowing down less while **laden**;
  - perhaps a healer;
  - an exploding species (**Exploding Ant**).
  - a huge Tier 3 brute whose every hit damages enemies in an area, like StarCraft II's Ultralisk (**Dinosaur Ant**). *[Added 2026-10-10.]*
- The person welcomes other ideas.
- The population limit (**pop cap**) is 300 per player. Stronger species have a higher **pop cost**.
- Colonies must build to raise their **housing** towards the pop cap, as with houses in Age of Empires II. Queens give some housing, and **brood cells** give more. *[Added 2026-10-10.]*
- Ants may overlap a little, as real ants do, but not so much that the battlefield becomes hard to read.
- Each kind of thing has a different shape, as well as the player's color.

### 7. Queens, hatcheries, brood cells and building

- There are no buildings. **Queens**, **hatcheries** and **brood cells** fill that role.
- **Queens** are the only places that train the basic (Tier 1) species, and the only **drop-offs**. A colony starts with one queen, which trains a melee ant, a gatherer and perhaps one more basic species.
- Every other species needs its own **hatchery** (for example an Acid Ant Hatchery). Training still happens from anywhere through the action grid; the hatchery only has to exist. *[The person suggested calling these "chambers"; "chamber" already means a room in the map, and "nursery" is too close to Nurse Ant, so "hatchery" is proposed instead. See the glossary.]*
- **Brood cells** add housing.
- The player chooses where every **structure** (queens, hatcheries, brood cells, walls) goes by placing **blueprints**.
- **Builders** (Carpenter Ants) carry the resources to each blueprint and build it. They are never given waypoints; they go to blueprints by themselves.
- Walls built by players can be destroyed.

### 8. Progression: tiers and upgrades

- There are three **tiers**, like the ages in Age of Empires II. A colony pays resources to **evolve** to the next tier, which unlocks stronger species and upgrades.
- **Upgrades** apply to the whole colony. Ants are never upgraded one at a time.
- Upgrades include **range**, **damage**, **health** and **armor**, as well as gather rate.
- Not every species needs every upgrade. Some upgrades affect all species (**colony upgrades**).

### 9. Combat

- Armor is flat: damage taken = damage − armor. For example, an attack of 12 damage against 8 armor deals 4. There is no percentage reduction.

### 10. Player actions and control

Player actions are global. There is no selecting of particular ants or queens.

1. **Set waypoints** for each species ("all acid ants go here").
   - Right-clicking a waypoint **clears** it.
   - When a species has several waypoints, its ants split evenly between them.
2. **Place blueprints** for queens and walls.
3. **Train** ants from anywhere, without going to a queen.
   - Ctrl with the button or hotkey trains 10 (**Train 10**).
   - Shift trains as many as the stockpile allows (**Train Max**).
4. **Upgrade**: colony-wide, as described in section 8.
5. **Retreat**: a toggle that pulls the colony's ants back towards its nests.

### 11. Interface and controls

- The whole game can be played with the mouse alone, and every action also has a **hotkey**.
- Actions sit in an **action grid** at the bottom left, shaped like the keyboard:
  - with 12 actions *[the request says "2", read here as 12]*, the grid has 3 rows and 4 columns, mapped as `Q W E R` / `A S D F` / `Z X C V`;
  - the grid is drawn with the stagger of a real keyboard.
- What is being trained or researched appears as a row of icons next to the resource counts (the **queue bar**), as in Age of Empires II.
- On-screen text is labels and label–value pairs, not prose. *[Added 2026-10-10.]*
- The resource bar shows each resource's estimated **income**, for example "4 Food/s". *[Added 2026-10-10.]*
- Hovering a glossary term anywhere on screen shows its **term hint**, a one-sentence definition. For example, Armor: "Each point of Armor reduces damage taken by 1." *[Added 2026-10-10.]*

### 12. Fog of war

- **Fog of war** uses real line of sight: vision is blocked by corners, rock and dirt, rather than being a plain circle.

### 13. Map events

- Dynamic, random **events** keep players alert. Examples: a new drop of resources (**windfall**), and a king-of-the-hill zone (**hill**).

### 14. Menus and settings

- The **main menu** offers:
  - **Play vs AI**;
  - **Online Multiplayer**, which also supports AI players;
  - **Match Settings**, which set the defaults;
  - **Player Settings**, such as remapping keys.
- Match settings include a large number of options, for example: chamber size, tunnel width, resource density, disabling species, game speed and starting resources.
- An online **lobby** can change the same match settings.
- The menu is ant-themed. For example, each button is a leaf, and choosing it makes an ant drag the leaf to one side.

### 15. Graphics

- Simple graphics are fine.
- Smooth, not tiled, like Age of Mythology rather than Age of Empires II. Tunnel and chamber edges are drawn as smooth, organic shapes, ants move continuously, and no tile grid is ever visible. A fine grid, about one ant per cell, is allowed inside the core for pathfinding and fog only. *[Added 2026-10-10.]*
- No health bars. Ants show no health. Each structure shows its health as a **health pie**: its player-color fill shrinks like a pie chart as health falls. *[Added 2026-10-10.]*
- The look evokes the setting (a picnic or a flower field).
- Everything a player owns shows that player's color, and each kind of thing has its own shape.

### 16. Performance

- Performance matters: there will be many small ants, each finding its own path.
- The target is 300 ants per player, which means 1,200 ants in a four-player match.

### 17. Later, not in the first version

- A campaign.
- AoE2-style civilizations, each with its own unique species, bonuses and upgrades.

### Decisions after the request

The person answered 1–3 on 2026-10-10. For 4–12 the person asked the agent to choose; each of those can be overridden.

| # | Question | Decision | By |
| --- | --- | --- | --- |
| 1 | How does a colony lose? | When it has nothing left that is an ant or can make ants: every ant and queen has died and every hatchery is destroyed. Walls, brood cells and blueprints do not keep a colony alive (confirmed by the person). A player can also **concede** at any time. | person |
| 2 | Who carries resources to blueprints? | Builders (Carpenter Ants). | person |
| 3 | What is a drop-off? | Only queens. A gatherer delivers by bringing its **inventory** to a queen. | person |
| 4 | How is a nest founded? | By placing a queen blueprint in a chamber, which Carpenter Ants then build. | agent |
| 5 | Who can dig dirt? | Any combat ant, by attacking it. Dirt has health. | agent |
| 6 | What can Flying Ants cross? | Walls only. Rock and dirt still block them, so the map's shape still matters. | agent |
| 7 | Minimum damage? | 1 per hit, even when armor is at least as high as damage. | agent |
| 8 | What happens when Retreat is switched off? | Ants return to their waypoints. | agent |
| 9 | Where does a species with no waypoint go? | Its ants wait at the nearest nest. | agent |
| 10 | What do 2v2 allies share? | Vision, but not resources. | agent |
| 11 | What if there are more actions than buttons? | Grid tabs. The layout is settled in the plan. | agent |
| 12 | Online play and Builder | Option (a): players find each other through public Nostr relays and STUN, which are contacted only after a player presses Host or Join. Loading the game and playing vs AI stay fully on the game's own origin, so R7 holds. The CSP default D4 is relaxed for the online path only, and the plan says exactly how. The netcode follows sgilson7's multiplayer games, adapted to this game, and is the only thing taken from them. TURN is decided later. | person |

## The tool

- **What:** a macro-focused real-time strategy game about ant colonies. 1v1, 2v2 and Free-for-all, against AI or online.
- **Who:** the person and friends, plus anyone the link is shared with.
- **Device:** a desktop or laptop browser with a mouse and keyboard. Phones and touch screens are not targeted.

## Done means

The conditions the person will check before deploying. Make each one checkable.

- [ ] The game launches in a desktop browser from the built static files, in Chromium, Firefox and WebKit.
- [ ] A complete match can be played to Victory or Defeat in Play vs AI.
- [ ] A complete match can be played online between two browsers.
- [ ] Every feature in "The request, by theme" and "Decisions after the request" is implemented, except section 17 ("Later, not in the first version").
- [ ] AI players have three difficulties, **Larva**, **Pupa** and **Imago**. In simulated matches, Imago beats Pupa and Pupa beats Larva most of the time.
- [ ] `cargo test` covers every core rule, and every behaviour test was broken once and seen to fail (recorded in `SECOND-ORDER.md`).
- [ ] `./scripts/check.sh` passes, and the result is recorded in `HANDOFF.md`.
- [ ] The game is documented: `README.md` (how to build, run and play), `GLOSSARY.md` (approved and current), and `HANDOFF.md`.
- [ ] All on-screen text follows `GLOSSARY.md`: glossary words only, label–value pairs, and term hints.
- [ ] 300 ants per colony (1,200 in Free-for-all) run at a playable frame rate. The plan sets the number and how it is measured.

## Decisions that stay with the person

Content, data, anything pedagogical, and anything outside `BUILDER-SPEC.md`.

- 

## Autonomy

One paragraph: what the agent may do alone, what it must ask about, and what it may never do. State here whether the plan is approved in advance.

The person chose autonomous mode on 2026-10-10.

- **Alone:** write and refactor code, tests and docs; choose numbers and balance within the plan; add glossary terms (marked proposed); commit locally.
- **Ask first:**
  - new core dependencies;
  - any change to a Builder requirement or default;
  - dropping, postponing or reshaping a feature in this brief;
  - renaming an approved glossary term;
  - anything that costs money or uses a new third-party service, such as TURN.
- **Never:** deploy; push to a remote without being asked; loosen a check to make it pass.
- **Plan approved in advance:** yes. The agent writes `PLAN.md` and continues building without waiting for the person to read it. Every other "Ask first" item above still applies.

## Allowed core dependencies

`serde`, `serde_json` (reference default). Add others only here, with the reason.
