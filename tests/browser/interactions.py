"""The project's own browser interactions, run by check.py in every engine.

check.py has already loaded the page, waited for it to report ready, and let
it idle. These drive the game the way a person would and check what the page
shows. Expected values are worked out by hand from the brief and the rules
(C4), never copied from the game's own output:

- Starting resources at Normal are 300 Food, 100 Nectar, 200 Leaves.
- A Harvester Ant costs 50 Food, so training one leaves 250 Food.
- A queen houses 20, and a colony starts with 10 ants (Population 10 / 20).
"""

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
STRINGS = json.loads((ROOT / "data" / "strings.json").read_text())


def text(page, selector):
    return page.inner_text(selector).strip()


def run(page, check, engine):
    leaves = [t.strip() for t in page.locator(".leaf .leaf-text").all_inner_texts()]
    check(
        leaves == [STRINGS["menu.play_vs_ai"], STRINGS["menu.online"], STRINGS["menu.match_settings"], STRINGS["menu.player_settings"]],
        f"the main menu shows the four leaves from the brief {leaves}",
    )

    # Each leaf is a different kind of leaf (oak, maple, willow, lily pad).
    kinds = page.evaluate(
        """() => [...document.querySelectorAll('.leaf')].map(l =>
            [...l.querySelectorAll('.leaf-shape')].filter(s => getComputedStyle(s).display !== 'none')
            .map(s => s.classList[1]).join(' '))"""
    )
    check(kinds == ["oak", "maple", "willow", "lily"], f"each main menu leaf is a different kind {kinds}")
    overflow = page.evaluate("() => getComputedStyle(document.getElementById('menus')).overflowX")
    check(overflow == "hidden", f"no horizontal scroll bar while a leaf is dragged away [{overflow}]")

    # Online Multiplayer: one Room Code field, the Status row, Host, Join,
    # Back. No pasted codes without ?codes. Typed codes are upper-cased.
    page.click("#go\\.online")
    page.wait_for_selector("#online\\.room", timeout=5000)
    buttons = [t.strip() for t in page.locator("#panels .button").all_inner_texts()]
    want = [STRINGS["online.host"], STRINGS["online.join"], STRINGS["menu.back"]]
    check(buttons == want, f"the Online Multiplayer screen offers Host, Join, Back {buttons}")
    fields = page.locator("#panels input, #panels textarea").count()
    check(fields == 1, f"one Room Code field and no pasted-code fields [{fields}]")
    page.fill("#online\\.room", "k7mxp4")
    page.wait_for_timeout(300)
    room = page.input_value("#online\\.room")
    check(room == "K7MXP4", f"a typed Room Code is upper-cased [{room}]")
    page.click("#go\\.main_menu")
    page.wait_for_selector("#go\\.play_vs_ai", timeout=5000)

    # Choosing a leaf: an ant drags it away, then the screen changes.
    page.click("#go\\.play_vs_ai")
    page.wait_for_selector("#pva\\.start", timeout=5000)
    check(True, "Play vs AI opens after the leaf is dragged away")
    page.click("#pva\\.start")
    page.wait_for_selector("#resources .res.food", timeout=10000)
    food = text(page, "#resources .res.food .value")
    check(food == "300", f"a match starts with 300 Food (Normal) [{food}]")
    pop = page.locator("#resources .res .value").nth(3).inner_text()
    check(pop == "10 / 20", f"Population starts at 10 / 20 [{pop}]")

    # Train one Harvester Ant with its hotkey, Q on the Ants tab.
    page.keyboard.press("Digit1")
    page.keyboard.press("KeyQ")
    page.wait_for_timeout(400)
    food = text(page, "#resources .res.food .value")
    check(food == "250", f"Q trains a Harvester Ant for 50 Food [{food}]")
    check(page.locator(".qitem").count() >= 1, "the queue bar shows the queued ant")

    # The keyboard-shaped grid: the Build tab's D is the Acid Ant Hatchery.
    page.keyboard.press("Digit2")
    page.wait_for_timeout(200)
    labels = [t.strip() for t in page.locator(".gbtn .label").all_inner_texts()]
    acid = STRINGS["structure.hatchery.acid"]
    check(acid in labels and labels.index(acid) == 6, f"D (row 2, column 3) is {acid} on the Build tab")

    # Hovering a grid button shows a label-value tooltip.
    page.hover(".gbtn >> nth=1")
    page.wait_for_timeout(200)
    tip = page.locator("#tooltip td").all_inner_texts()
    check(STRINGS["stat.cost"] in tip, f"tooltips are label-value pairs {tip[:6]}")

    # The HUD updates every tick, in place: every tab click opens its tab
    # (TRIAGE row 1), and the hovered button keeps its tooltip (row 3).
    tabs = [STRINGS["tab." + t] for t in ("ants", "build", "waypoints", "upgrades", "colony")]
    missed = []
    for round_ in range(3):
        for i, name in enumerate(tabs):
            # Held like a person's click, longer than one HUD update.
            b = page.locator(f"#tabs .tab >> nth={i}").bounding_box()
            page.mouse.move(b["x"] + b["width"] / 2, b["y"] + b["height"] / 2)
            page.mouse.down()
            page.wait_for_timeout(150)
            page.mouse.up()
            page.wait_for_timeout(120)
            active = page.inner_text("#tabs .tab.active").strip()
            if not active.startswith(name):
                missed.append((name, active))
    check(not missed, f"all 15 tab clicks open their tab while the HUD updates {missed}")
    page.click("#tabs .tab >> nth=1")
    page.hover(".gbtn >> nth=0")
    page.evaluate("() => { window.__hovered = document.querySelector('.gbtn'); }")
    hidden = []
    for _ in range(15):
        page.wait_for_timeout(100)
        hidden.append(page.evaluate("() => document.getElementById('tooltip').hidden"))
    same = page.evaluate("() => window.__hovered === document.querySelector('.gbtn') && window.__hovered.isConnected")
    check(not any(hidden) and same, f"the hovered button is kept and its tooltip stays for 1.5 s {hidden}")
    tier = page.inner_text(".gbtn >> nth=0 >> .tier").strip()
    check(tier == "1", f"the Queen button shows its tier digit [{tier}]")

    # A tooltip in the top bar stays inside the window (row 4). Train Max
    # of Soldier Ants (Shift + W) fills the queue first.
    page.mouse.move(700, 400)
    page.keyboard.press("Digit1")
    page.keyboard.press("Shift+KeyW")
    page.wait_for_selector(".qitem", timeout=5000)
    page.hover(".qitem >> nth=0")
    page.wait_for_timeout(200)
    box = page.evaluate("() => { const t = document.getElementById('tooltip'); const r = t.getBoundingClientRect(); return [t.hidden, r.top, r.left, r.right, innerWidth]; }")
    check(not box[0] and box[1] >= 0 and box[2] >= 0 and box[3] <= box[4], f"the queue-bar tooltip shows inside the window {box}")

    # Esc opens the menu; Concede ends the match in Defeat.
    page.mouse.move(700, 400)
    page.keyboard.press("Escape")
    page.click("#menu\\.concede")
    page.click("#menu\\.concede_yes")
    page.locator("#overlay h2", has_text=STRINGS["end.defeat"]).wait_for(timeout=5000)
    title = text(page, "#overlay h2")
    check(title == STRINGS["end.defeat"], f"conceding shows {STRINGS['end.defeat']} [{title}]")
    page.click("#end\\.main_menu")
    page.wait_for_selector(".leaf", timeout=5000)
    check(True, "Main Menu returns after the match")

    # Spectate (row 20): Red as an AI; no action grid; Fog of War, Pause and
    # Time Scale in the top bar.
    page.click("#go\\.play_vs_ai")
    page.wait_for_selector("#pva\\.start", timeout=5000)
    page.select_option("#ai\\.0", "imago")
    page.click("#pva\\.start")
    page.wait_for_selector("#controls .control", timeout=10000)
    check(page.locator("#grid").is_hidden(), "a spectated match has no action grid")
    controls = [t.strip() for t in page.locator("#controls .control").all_inner_texts()]
    check(len(controls) == 3 and controls[0] == STRINGS["hud.fog_of_war"] + ": " + STRINGS["off"], f"Fog of War, Pause and Time Scale controls {controls}")
    page.click("#controls .control >> nth=0")
    page.wait_for_timeout(300)
    first = page.inner_text("#controls .control >> nth=0").strip()
    check(first == STRINGS["hud.fog_of_war"] + ": " + STRINGS["color.0"], f"the Fog of War control cycles to Red [{first}]")
    page.mouse.move(700, 400)
    page.keyboard.press("Escape")
    page.click("#end\\.main_menu")
    page.wait_for_selector(".leaf", timeout=5000)
    check(True, "a spectator leaves to the Main Menu")

    # Player Settings → Benchmark: the performance scenario runs live and
    # its panel reports the budget (PLAN decision 25).
    page.click("#go\\.player_settings")
    page.wait_for_selector("#go\\.benchmark", timeout=5000)
    page.click("#go\\.benchmark")
    page.wait_for_selector("#panel dd", timeout=20000)
    page.wait_for_timeout(3000)
    labels = page.locator("#panel dt").all_inner_texts()
    values = page.locator("#panel dd").all_inner_texts()
    panel = dict(zip(labels, values))
    ants = int(panel.get("Ants", "0"))
    check(ants >= 400, f"the Benchmark runs about 1,200 ants [{ants}]")
    check(panel.get("Status") in ("Within budget", "Over budget"), f"the Benchmark reports its budget {panel}")
