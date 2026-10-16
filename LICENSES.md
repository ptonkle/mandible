# Where everything comes from

Mandible's own code and data are under the MIT licence in [`LICENSE`](LICENSE).

| What | Where it comes from | Licence |
| --- | --- | --- |
| Game code (`crates/`, `web/` except `web/vendor/`, `scripts/`, `tests/`) | Written in this repository | MIT, [`LICENSE`](LICENSE) |
| Text and game data (`data/*.json`) | Written in this repository | MIT, [`LICENSE`](LICENSE) |
| Graphics | None as files: the map, ants and buildings are drawn on a canvas by `web/render.js` at run time | MIT, [`LICENSE`](LICENSE) |
| Sound | None as files: every sound is synthesised with the Web Audio API in `web/app.js` | MIT, [`LICENSE`](LICENSE) |
| Fonts | None bundled: the page uses the browser's own system fonts (`ui-monospace`, `monospace` and the default sans-serif) | n/a |
| `web/vendor/trystero-nostr-0.25.4.js` | [Trystero](https://github.com/dmotz/trystero) 0.25.4 by Dan Motzenbecker, Nostr strategy (`@trystero-p2p/nostr@0.25.4`), bundled by esm.sh; pinned by sha256 in `web/vendor/SHA256SUMS` and not edited | MIT, Copyright (c) Dan Motzenbecker |
| Code inside that bundle | [noble-secp256k1](https://github.com/paulmillr/noble-secp256k1) by Paul Miller, included by Trystero | MIT, Copyright (c) 2019 Paul Miller |
| Rust crates (`Cargo.lock`) | crates.io: `wasm-bindgen`, `serde`, `serde_json` and their dependencies (some only at build time) | MIT or Apache-2.0 (each crate's own licence) |

Online play connects, only after a player presses Host or Join, to public Nostr relays (listed in `web/online-config.js`) and Google's public STUN servers. Nothing from them is shipped in this repository.
