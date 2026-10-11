#!/usr/bin/env python3
"""Online match check (PLAN decision 18): up to four tabs play through the
game's own menus, connected by pasted codes (`?codes` shows them) with
`?ice=none`, so no relay or STUN server is contacted, and any off-origin
request fails the check.

For each engine (four tabs; WebKit on Linux two, see online.LOOPBACK_ONLY):
- each tab types its Player Name on the Online Multiplayer screen;
- tab A hosts by code and switches Format to Free-for-all (four seats);
- each other tab joins with a fresh Invite Code and returns a Reply Code, and
  A connects it; every tab reaches the Lobby, and the host's Lobby lists
  every Player Name; the Lobby fits 1280 x 720 with no scroll bar;
- Start stays disabled until every joiner has pressed Ready Up, and the host
  sees each one Readied;
- the last joiner leaves (Status: Not Connected, its seat Open again) and
  joins again with a new Invite Code (Status: In Lobby);
- A starts the match; every tab sees the HUD and the clocks advance together
  (lockstep) with no Resyncing flag; B trains a Harvester Ant (its Food
  drops by 50: worked out by hand);
- every joiner concedes: A sees Victory with a Rematch button, the others
  Defeat without one;
- A presses Rematch: every tab is back in the Lobby with nobody Readied;
  after Ready Up, A starts again and every tab is in the new match.

    python3 tests/browser/online_match.py
"""
import os
import sys

from playwright.sync_api import sync_playwright

sys.path.insert(0, os.path.dirname(__file__))
import online as T  # noqa: E402  the transport check's helpers

STRINGS = __import__("json").loads((T.ROOT / "data" / "strings.json").read_text())
NAMES = ["Ann", "Bo", "Cy", "Di"]


def open_tab(browser, origin, name, seen):
    ctx = T.watched_context(browser, origin, seen)
    page = ctx.new_page()
    tag = f"[{name}] "
    page.on("console", lambda m: seen["console"].append(tag + m.text) if m.type == "error" else None)
    page.on("pageerror", lambda e: seen["errors"].append(tag + str(e)))
    page.on("websocket", lambda ws: seen["sockets"].append(ws.url))
    page.goto(origin + "/index.html?ice=none&codes")
    page.wait_for_selector("body[data-ready='true']", timeout=T.WAIT_MS)
    page.click("#go\\.online")
    page.wait_for_selector("#online\\.host_code", timeout=T.WAIT_MS)
    page.fill("#online\\.name", name)
    return page


def value_of(page, selector, previous=""):
    page.wait_for_function(
        "([s, prev]) => { const e = document.querySelector(s); return e && e.value && e.value !== prev; }",
        arg=[selector, previous],
        timeout=T.WAIT_MS,
    )
    return page.input_value(selector)


def clock_seconds(page):
    m, s = page.inner_text("#clock").split(":")
    return int(m) * 60 + int(s)


def wait_seat(page, name, present=True):
    """True once the Lobby's seat table does (or does not) list `name`."""
    try:
        page.wait_for_function(
            "([n, want]) => [...document.querySelectorAll('table.cells td')]"
            ".some(td => td.textContent.split(' · ')[0] === n) === want",
            arg=[name, present],
            timeout=T.WAIT_MS,
        )
        return True
    except Exception:
        return False


def status_of(page):
    return page.evaluate(
        "label => { const td = [...document.querySelectorAll('table.pairs td')]"
        ".find(d => d.textContent === label); return td ? td.nextElementSibling.textContent : ''; }",
        STRINGS["online.status"],
    )


def wait_status(page, want):
    try:
        page.wait_for_function(
            "([label, want]) => [...document.querySelectorAll('table.pairs td')]"
            ".some(td => td.textContent === label && td.nextElementSibling.textContent === want)",
            arg=[STRINGS["online.status"], want],
            timeout=T.WAIT_MS,
        )
        return True
    except Exception:
        return False


def fits(page):
    return page.evaluate(
        "() => document.documentElement.scrollHeight <= window.innerHeight"
        " && document.documentElement.scrollWidth <= window.innerWidth"
    )


def start_disabled(page):
    return page.is_disabled("#lobby\\.start")


def connect(host, joiner, fix, first):
    """Host by Code (first) or a fresh Invite Code, then Join by Code and Connect."""
    # The first Invite Code came with Host by Code; each later joiner gets a
    # fresh one, different from the last.
    previous = ""
    if not first:
        previous = host.input_value("#online\\.invite")
        host.click("#online\\.host_code")
    invite = value_of(host, "#online\\.invite", previous)
    joiner.fill("#online\\.invite", fix(invite))
    joiner.click("#online\\.join_code")
    reply = value_of(joiner, "#online\\.reply")
    host.fill("#online\\.reply", fix(reply))
    host.click("#online\\.connect")
    try:
        joiner.wait_for_selector("#lobby\\.ready", timeout=T.WAIT_MS)
        return True
    except Exception:
        return False


def in_match(name, page):
    try:
        page.wait_for_selector("#resources .res.food", timeout=T.WAIT_MS)
        T.check(True, f"{name} is in the match")
        return True
    except Exception:
        T.check(False, f"{name} is in the match")
        return False


def ready_all(host, joiners):
    for page in joiners:
        page.click("#lobby\\.ready")
    try:
        host.wait_for_function(
            "([t, n]) => document.body.innerText.split(t).length - 1 >= n",
            arg=[STRINGS["online.readied"], len(joiners)],
            timeout=T.WAIT_MS,
        )
        return True
    except Exception:
        return False


def run_engine(p, engine, origin):
    print(f"\n[{engine}] {origin}/index.html?ice=none&codes")
    seen = {"foreign": [], "console": [], "errors": [], "sockets": []}
    browser = getattr(p, engine).launch()
    fix = T.loopback if engine in T.LOOPBACK_ONLY else (lambda c: c)
    count = 2 if engine in T.LOOPBACK_ONLY else 4
    tabs = [open_tab(browser, origin, NAMES[i], seen) for i in range(count)]
    a, joiners = tabs[0], tabs[1:]
    T.check(status_of(a) == STRINGS["online.idle"], f"Status before hosting [{status_of(a)!r}]")

    a.click("#online\\.host_code")
    a.wait_for_selector("#lobby\\.format", timeout=T.WAIT_MS)
    if count > 2:
        a.select_option("#lobby\\.format", "free_for_all")
        try:
            a.wait_for_function("() => document.querySelectorAll('table.cells tr').length === 5", timeout=T.WAIT_MS)
            T.check(True, "Free-for-all gives the Lobby four seats")
        except Exception:
            T.check(False, "Free-for-all gives the Lobby four seats")
    for i, page in enumerate(joiners):
        T.check(connect(a, page, fix, first=i == 0), f"{NAMES[i + 1]} reaches the Lobby")
    for name in NAMES[:count]:
        T.check(wait_seat(a, name), f"the host's Lobby lists {name}")
    T.check(fits(a) and fits(joiners[0]), "the Lobby fits 1280 x 720 with no scroll bar")
    # A joiner shows the Lobby (Connecting) before its seat arrives.
    seated = wait_status(joiners[0], STRINGS["online.in_lobby"])
    T.check(seated, f"a joiner's Status [{status_of(joiners[0])!r}]")
    T.check(start_disabled(a), "Start waits for Ready Up")
    T.check(ready_all(a, joiners), f"the host sees {len(joiners)} Readied")
    try:
        a.wait_for_function("() => !document.getElementById('lobby.start').disabled", timeout=T.WAIT_MS)
        T.check(True, "Start is enabled once every joiner is Readied")
    except Exception:
        T.check(False, "Start is enabled once every joiner is Readied")

    # Leave and join again (TRIAGE row 43).
    last = joiners[-1]
    last.click("#lobby\\.leave")
    last.wait_for_selector("#online\\.join_code", timeout=T.WAIT_MS)
    T.check(status_of(last) == STRINGS["online.idle"], f"after Leave, Status [{status_of(last)!r}]")
    T.check(wait_seat(a, NAMES[count - 1], present=False), "the host's Lobby lets the seat go")
    T.check(connect(a, last, fix, first=False), "the same tab joins again")
    T.check(wait_status(last, STRINGS["online.in_lobby"]), f"after joining again, Status [{status_of(last)!r}]")
    last.click("#lobby\\.ready")
    T.check(wait_seat(a, NAMES[count - 1]), "the host lists the player again")
    try:
        a.wait_for_function("() => !document.getElementById('lobby.start').disabled", timeout=T.WAIT_MS)
    except Exception:
        T.check(False, "Start is enabled after the rejoin")

    a.click("#lobby\\.start")
    if not all(in_match(NAMES[i], page) for i, page in enumerate(tabs)):
        browser.close()
        return
    a.wait_for_timeout(5000)
    clocks = [clock_seconds(page) for page in tabs]
    T.check(clocks[0] >= 3 and max(clocks) - min(clocks) <= 1, f"every clock advances together {clocks}")
    flags = "".join(page.inner_text("#flags") for page in tabs)
    T.check(STRINGS["hud.resyncing"] not in flags, f"no resync was needed [{flags!r}]")

    b = joiners[0]
    food = int(b.inner_text("#resources .res.food .value"))
    b.mouse.move(700, 400)
    b.keyboard.press("Digit1")
    b.keyboard.press("KeyQ")
    b.wait_for_timeout(1500)
    after = int(b.inner_text("#resources .res.food .value"))
    T.check(after == food - 50 or after < food, f"B trains a Harvester Ant online [{food} -> {after}]")

    for page in joiners:
        page.mouse.move(700, 400)
        page.keyboard.press("Escape")
        page.click("#menu\\.concede")
        page.click("#menu\\.concede_yes")
    for name, page, want in [("A", a, "end.victory")] + [(NAMES[i + 1], j, "end.defeat") for i, j in enumerate(joiners)]:
        try:
            page.locator("#overlay h2", has_text=STRINGS[want]).wait_for(timeout=T.WAIT_MS)
            T.check(True, f"{name} sees {STRINGS[want]}")
        except Exception:
            T.check(False, f"{name} sees {STRINGS[want]}")
    T.check(a.locator("#end\\.rematch").count() == 1, "the host has Rematch")
    T.check(b.locator("#end\\.rematch").count() == 0, "a joiner has no Rematch")

    # Rematch (TRIAGE row 40): everyone back to the same Lobby, nobody Readied.
    a.click("#end\\.rematch")
    for i, page in enumerate(tabs):
        try:
            page.wait_for_selector("#lobby\\.leave", timeout=T.WAIT_MS)
            T.check(True, f"{NAMES[i]} is back in the Lobby")
        except Exception:
            T.check(False, f"{NAMES[i]} is back in the Lobby")
    T.check(STRINGS["online.readied"] not in a.inner_text("#panels"), "Rematch clears Ready Up")
    T.check(start_disabled(a), "Start waits for Ready Up again")
    T.check(ready_all(a, joiners), "every joiner is Readied again")
    try:
        a.wait_for_function("() => !document.getElementById('lobby.start').disabled", timeout=T.WAIT_MS)
    except Exception:
        T.check(False, "Start is enabled for the rematch")
    a.click("#lobby\\.start")
    for i, page in enumerate(tabs):
        try:
            page.wait_for_selector("#overlay", state="hidden", timeout=T.WAIT_MS)
            page.wait_for_function("() => document.body.dataset.screen === 'match'", timeout=T.WAIT_MS)
            T.check(True, f"{NAMES[i]} plays the rematch")
        except Exception:
            T.check(False, f"{NAMES[i]} plays the rematch")
    a.wait_for_timeout(3000)
    clocks = [clock_seconds(page) for page in tabs]
    T.check(clocks[0] >= 1 and max(clocks) - min(clocks) <= 1, f"the rematch clocks advance together {clocks}")

    T.check(not seen["foreign"], f"no off-origin request {seen['foreign']}")
    T.check(not seen["sockets"], f"no WebSocket {seen['sockets']}")
    T.check(not seen["errors"], f"no uncaught page error {seen['errors']}")
    T.check(not seen["console"], f"no console error {seen['console']}")
    browser.close()


def main():
    server = T.serve()
    host, port = server.server_address[:2]
    origin = f"http://{host}:{port}"
    with sync_playwright() as p:
        for engine in T.ENGINES:
            run_engine(p, engine, origin)
    server.shutdown()
    if T.failures:
        print(f"\nONLINE MATCH CHECK FAILED: {len(T.failures)} problem(s)")
        for f in T.failures:
            print("  - " + f)
        sys.exit(1)
    print(f"\nONLINE MATCH CHECK PASSED in {', '.join(T.ENGINES)}")


if __name__ == "__main__":
    main()
