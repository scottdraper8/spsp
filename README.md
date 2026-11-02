# SPSP — Starship Power Setting Persistence

SPSP remembers the Starship Power Diversion mode selected for each player-owned ship. Changing
ships, summoning a ship, changing systems, and other game state transitions must not silently
replace a ship's chosen mode. Renaming a ship must not create a new identity.

SPSP must not edit, extend, or rewrite No Man's Sky save files. If vanilla data files cannot
express this behavior, the mod will keep a small, versioned sidecar in its own application-data
directory and apply it through the narrowest verified runtime integration. Any runtime integration
must fail closed on an unsupported game build; no guessed executable offsets or broad memory writes
are acceptable.

## Current technical boundary

The current No Man's Sky data-mod loader accepts loose data files and does not execute arbitrary
mod code. The inspected player-state template has owned/current ship resource identity but no
serialized power-diversion mode. Static analysis of the supported executable identifies the
`Ship_CyclePower` update site and maps its stored values to Balanced, Guns, Engine, and Shield.
This makes a data-only patch insufficient for per-ship persistence.

A Windows runtime candidate now checks the exact executable identity and expected code bytes. It
captures the selected value before the game stores it and has candidate restore hooks after owned-
ship spawn. Runtime activation remains explicitly disabled: the candidate restore write, ship/profile
identity stability, loader behavior, and transition handling still require independent validation.
The candidate ship key uses resource fields rather than the display name; collisions fail closed,
but stability across rename, save/reload, summon/switch, travel, and ownership-array changes is not
yet proven.

## Project status

The repository provides a bounded, versioned sidecar store with strict validation, candidate
name-independent identity derivation, mode-mapping tests, and a Windows runtime scaffold. The
runtime is inert until all bindings are validated; SPSP is not installable or release-ready and no
game build is claimed as supported. Do not deploy it to a live game until the runtime and
persistence gates in [AGENTS.md](AGENTS.md) and [the design contract](docs/design.md) are met.

## Development

Use the pinned Rust toolchain and exact dependency versions. From this directory:

```sh
python3 -B scripts/run_cargo.py fmt --all -- --check
python3 -B scripts/run_cargo.py clippy --workspace --all-targets --all-features --locked -- -D warnings
python3 -B scripts/run_cargo.py test --workspace --all-features --locked
pre-commit run --all-files
```

Install the repository's Git hooks before making changes:

```sh
pre-commit install --install-hooks
```

Pre-commit is part of the required development path, not an optional convenience. Its first run
fetches actionlint into a user-local cache and verifies the release archive against the pinned
SHA-256 before checking workflows. See [AGENTS.md](AGENTS.md) before changing code or CI policy.

## Game compatibility

No NMS game build is currently claimed as supported. Runtime compatibility will be declared only
for an exact public game build whose executable identity, hook signatures, ship identity, loading,
state transitions, and teardown behavior have been independently verified. SPSP does not alter
save files and does not ship No Man's Sky assets.

## License and trademarks

SPSP source is licensed under [PolyForm Strict 1.0.0](LICENSE.txt). No Man's Sky and its assets are
property of Hello Games. SPSP is an unofficial, unsupported third-party mod and is not affiliated
with or endorsed by Hello Games.
