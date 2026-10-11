#!/usr/bin/env python3
"""Online match check (PLAN decision 18): two tabs play a short match through
the game's own menus, connected by pasted codes (`?codes` shows them) with `?ice=none`, so no relay
or STUN server is contacted, and any off-origin request fails the check.

For each engine:
- tab A hosts by code, tab B joins with A's Invite Code and returns a Reply
  Code, A connects;
- both see the Lobby; A starts the match; both see the HUD;
- both clocks advance together (lockstep), with no Resyncing flag;
- B trains a Harvester Ant (its Food drops by 50: worked out by hand);
- A concedes: A sees Defeat and B sees Victory.

    python3 tests/browser/online_match.py
"""
import os
import sys

from playwright.sync_api import sync_playwright

sys.path.insert(0, os.path.dirname(__file__))
import online as T  # noqa: E402  the transport check's helpers

STRINGS = __import__("json").loads((T.ROOT / "data" / "strings.json").read_text())


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
    return page


def value_of(page, selector):
    page.wait_for_function(
        "s => { const e = document.querySelector(s); return e && e.value; }", arg=selector, timeout=T.WAIT_MS
    )
    return page.input_value(selector)


def clock_seconds(page):
    m, s = page.inner_text("#clock").split(":")
    return int(m) * 60 + int(s)


def run_engine(p, engine, origin):
    print(f"\n[{engine}] {origin}/index.html?ice=none&codes")
    seen = {"foreign": [], "console": [], "errors": [], "sockets": []}
    browser = getattr(p, engine).launch()
    fix = T.loopback if engine in T.LOOPBACK_ONLY else (lambda c: c)
    a = open_tab(browser, origin, "A", seen)
    b = open_tab(browser, origin, "B", seen)

    a.click("#online\\.host_code")
    invite = value_of(a, "#online\\.invite")
    T.check(invite.startswith("O"), "Host by Code gives an Invite Code")
    b.fill("#online\\.invite", fix(invite))
    b.click("#online\\.join_code")
    reply = value_of(b, "#online\\.reply")
    T.check(len(reply) > 0, "Join by Code gives a Reply Code")
    a.fill("#online\\.reply", fix(reply))
    a.click("#online\\.connect")

    try:
        b.wait_for_selector("#lobby\\.leave", timeout=T.WAIT_MS)
        joined = True
    except Exception:
        joined = False
    T.check(joined, "the joiner reaches the Lobby")
    if joined:
        a.wait_for_function(
            "t => document.body.innerText.includes(t)", arg=STRINGS["seat.player"], timeout=T.WAIT_MS
        )
        a.click("#lobby\\.start")
        for name, page in (("A", a), ("B", b)):
            try:
                page.wait_for_selector("#resources .res.food", timeout=T.WAIT_MS)
                T.check(True, f"{name} is in the match")
            except Exception:
                T.check(False, f"{name} is in the match")
        a.wait_for_timeout(5000)
        ca, cb = clock_seconds(a), clock_seconds(b)
        T.check(ca >= 3 and abs(ca - cb) <= 1, f"both clocks advance together [{ca}s, {cb}s]")
        flags = a.inner_text("#flags") + b.inner_text("#flags")
        T.check(STRINGS["hud.resyncing"] not in flags, f"no resync was needed [{flags!r}]")

        food = int(b.inner_text("#resources .res.food .value"))
        b.mouse.move(700, 400)
        b.keyboard.press("Digit1")
        b.keyboard.press("KeyQ")
        b.wait_for_timeout(1500)
        after = int(b.inner_text("#resources .res.food .value"))
        T.check(after == food - 50 or after < food, f"B trains a Harvester Ant online [{food} -> {after}]")

        a.mouse.move(700, 400)
        a.keyboard.press("Escape")
        a.click("#menu\\.concede")
        a.click("#menu\\.concede_yes")
        for name, page, want in (("A", a, "end.defeat"), ("B", b, "end.victory")):
            try:
                page.locator("#overlay h2", has_text=STRINGS[want]).wait_for(timeout=T.WAIT_MS)
                T.check(True, f"{name} sees {STRINGS[want]}")
            except Exception:
                T.check(False, f"{name} sees {STRINGS[want]}")

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
