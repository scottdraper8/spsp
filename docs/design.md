# Design contract

## State that belongs to SPSP

SPSP tracks one of the four vanilla power modes—balanced, engine, guns, or shield—for each
positively identified player-owned ship. Display names are mutable and never participate in a key.
The source of truth is a small, versioned sidecar owned by the SPSP mod, not a save-file field.

The sidecar format must be bounded, validated, and deterministic. Writes occur only when a player
explicitly changes a mode. Use an atomic replace in the same directory; a failed write must leave the
previous valid file intact. Invalid, oversized, duplicate, or unknown-version data must not be
silently normalized into a destructive rewrite. Keep diagnostics useful without logging player or
ship identifiers.

The file must be scoped to the active save profile as well as the ship. The runtime must prove how
to obtain a stable profile identity before implementing that scope. If it cannot do so safely, it
must decline to apply state rather than risk mixing ships from separate saves.

The current Rust store implements a bounded schema-v1 JSON sidecar (256 KiB maximum, up to 16
profiles and 64 ships per profile), canonical deterministic encoding, strict duplicate and schema
validation, and same-directory atomic replacement. Profile and ship keys are domain-separated,
length-prefixed SHA-256 hashes of caller-supplied stable identity fields; raw identity components
are not written to the sidecar. This hash is not encryption, and the candidate game fields remain
unverified. The store refuses to replace a pre-existing invalid sidecar so recovery data is not
silently destroyed.

## Runtime responsibilities

The runtime has two narrow responsibilities:

1. Observe the vanilla player action that changes power mode and persist the selected mode for the
   identified active player ship before a later transition can discard it.
2. When a player-owned ship becomes active, resolve its verified identity and restore its recorded
   mode using the game's own mode-changing path. Avoid reapplying unchanged state or intercepting
   unrelated ship classes, AI ships, menus, or multiplayer-owned entities.

The integration must be version-gated before installing any hook. Each consumed address, signature,
structure field, and calling convention needs an exact-build evidence record and a fail-closed test.
Hooks must have bounded lifetime, safe teardown, and no disk access in per-frame callbacks.

## Identity contract

Current serialized templates expose a `GcPlayerOwnershipData.Resource` value containing resource
filename, procedural seed, and alternate ID fields, and expose the player's owned/current ships.
These are candidates for a name-independent key, not yet a proven unique runtime identity. Before
implementation, determine whether these fields remain stable across rename, summon, system travel,
reload, and ownership-array reorder, and whether two legitimate ships can share the candidate key.

If identity collisions are possible, identify a stronger stable field or fail closed for collisions.
Never fall back to ship names, array positions alone, model appearance, or a guessed pointer
lifetime.

## Compatibility and release gates

- A data-only EXML/MBIN solution is insufficient unless new evidence demonstrates a vanilla data
  mechanism that both observes player changes and stores independent per-ship state outside saves.
- Runtime code is optional only if the data-only alternative proves all product acceptance cases.
  Otherwise, ship a minimal exact-build-gated runtime component; do not claim broad game-version
  support.
- A release requires unit/integration coverage for the sidecar and transition model, Windows target
  build validation, a static executable-binding audit for each supported build, and controlled
  in-game checks of change/restore/rename/switch/summon/travel/reload behavior.
- No game save is modified by the implementation or its test harness. Any live behavior test uses a
  disposable save under a controlled environment.
