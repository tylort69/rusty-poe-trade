# Rusty POE Trade

A Path of Exile price-check overlay for **Arch Linux** and **Windows 11**, written in Rust.
It's a rewrite of [Awakened PoE Trade](https://github.com/SnosMe/awakened-poe-trade) with a
Material You interface that takes its colors from your desktop accent (or a color you pick).

> This product isn't affiliated with or endorsed by Grinding Gear Games in any way.

## Features

Everything from Awakened PoE Trade:

- **Price check** (Ctrl+D): trade-site listings, bulk exchange, poe.ninja prices and trends,
  poeprices.info predictions, stat filters with presets.
- **Item check** (set a hotkey in Settings): wiki / poedb / Craft of Exile links, map mod warnings, DPS, dust value.
- Item search, stash search, chat commands, stopwatch, image strips, the delve grid.
- Imports an existing Awakened PoE Trade `config.json` on first run.

Added in Rusty POE Trade:

- **Account sign-in** with your POESESSID session cookie today. Official GGG OAuth is built in
  and turns on once a client ID is available. Secrets live only in your OS keychain.
- **Wealth**: stash valuation, net-worth history, dust calculator, bulk-sell list priced at a
  percentage of poe.ninja.
- **Trade whispers**: incoming and outgoing trades with Invite, Trade, Thanks and Kick buttons.
- **Mapping stats**: map times, maps per hour, deaths and history, read from `Client.txt`.
- **Market**: 7-day economy movers, favorites, saved trade searches, price-history charts.
- **Item inspect**: each mod's tier and where each roll sits in its range.
- **Path of Building viewer**: open local builds or pobb.in / pastebin codes.
- **Overlay zoom** (50–300%) and ultrawide-aware widget placement.

Everything runs locally. Rusty POE Trade only talks to pathofexile.com, poe.ninja,
poeprices.info and, if you import a build, pobb.in / pastebin.

## Install

### Arch Linux

```bash
git clone https://github.com/tylort69/rusty-poe-trade
cd rusty-poe-trade/packaging/arch && makepkg -si
```

The overlay runs as an X11 (XWayland) window, so it works under Wayland compositors too. On
Hyprland, keep Path of Exile fullscreen so the overlay can sit on top of it. To remember your
sign-in you need a Secret Service keychain, such as `gnome-keyring` or KWallet.

### Windows 11

Download the installer or the portable zip from
[Releases](https://github.com/tylort69/rusty-poe-trade/releases). If Path of Exile runs as
administrator, run Rusty POE Trade as administrator too, so its hotkeys reach the game.

## Usage

Press **Shift+Space** to open the overlay. Settings are in the gear icon of the top-left menu.

To value your stash, open **Settings → Account** and sign in. To get your POESESSID, sign in at
pathofexile.com, press F12, and copy the `POESESSID` cookie. Treat it like a password.

## Building from source

Requires Rust 1.88 or newer. On Linux you also need the X11, xdotool and D-Bus development
packages.

```bash
cargo build --release
cargo test --workspace
```

The workspace is split into crates:

| Crate | Purpose |
| --- | --- |
| `rpt-data` | Embedded game data (items, stats, client strings) |
| `rpt-parser` | Clipboard item parser |
| `rpt-trade` | Trade API, filters, poe.ninja, poeprices, rate limits |
| `rpt-stash` | Account sign-in (session cookie / OAuth) and stash API |
| `rpt-platform` | Hotkeys, game window tracking, clipboard, tray, log watcher, updater |
| `rpt-m3` | Material You (Material 3) components for egui |
| `app` | The overlay application |

## License

MIT. Based on Awakened PoE Trade by Alexander Drozdov, MIT licensed; see `LICENSE-UPSTREAM`.
