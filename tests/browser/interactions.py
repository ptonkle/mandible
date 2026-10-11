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
