# SPSP — Starship Power Setting Persistence

SPSP remembers the Starship Power Diversion mode selected for each player-owned ship. Changing
ships, summoning a ship, changing systems, and other game state transitions must not silently
replace a ship's chosen mode. Renaming a ship must not create a new identity.

SPSP must not edit, extend, or rewrite No Man's Sky save files. If vanilla data files cannot
express this behavior, the mod will keep a small, versioned sidecar in its own mod directory and
apply it through the narrowest verified runtime integration. Any runtime integration must fail
closed on an unsupported game build; no guessed executable offsets or broad memory writes are
acceptable.

## Current technical boundary

The current No Man's Sky data-mod loader accepts loose data files and does not execute arbitrary
mod code. The currently inspected player-state template has owned/current ship resource identity
but no serialized power-diversion mode. The game executable exposes the `Ship_CyclePower` action
and four power-setting labels (balanced, engine, guns, and shield). This makes a data-only patch
insufficient for per-ship persistence; a runtime seam still needs to be established and validated
before SPSP can be installed or called release-ready.

The candidate name-independent ship identity is the owned ship's resource identity, not its display
name. Its exact stable fields, duplicate handling, and save-profile scope remain runtime evidence
gates. SPSP must not persist a mode against an ambiguous identity.

## Project status

The repository currently provides a versioned, serializable power-mode model and the security,
quality, and dependency-pinning foundation for the implementation. It does not yet provide an
installable mod, a DLL, or runtime hooks. Do not deploy it to a live game until the runtime and
persistence gates in [AGENTS.md](AGENTS.md) and [the design contract](docs/design.md) are met.

## Development

Use the pinned Rust toolchain and exact dependency versions. From this directory:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
pre-commit run --all-files
```

Install the repository's Git hooks before making changes:

```sh
pre-commit install --install-hooks
```

Pre-commit is part of the required development path, not an optional convenience. See
[AGENTS.md](AGENTS.md) before changing code or CI policy.

## Game compatibility

No NMS game build is currently claimed as supported. Runtime compatibility will be declared only
for an exact public game build whose executable identity, hook signatures, ship identity, loading,
state transitions, and teardown behavior have been independently verified. SPSP does not alter
save files and does not ship No Man's Sky assets.

## License and trademarks

SPSP source is licensed under [PolyForm Strict 1.0.0](LICENSE.txt). No Man's Sky and its assets are
property of Hello Games. SPSP is an unofficial, unsupported third-party mod and is not affiliated
with or endorsed by Hello Games.
