# Design contract

## State that belongs to SPSP

SPSP tracks one of the four vanilla power modes—balanced, engine, guns, or shield—for each
positively identified player-owned ship. Display names are mutable and never participate in a key.
The source of truth is a small, versioned sidecar owned by the SPSP mod, not a save-file field.

The sidecar format must be bounded, validated, and deterministic. Writes occur only when a player
explicitly changes a mode. Use an atomic replace in the same directory; a failed write must leave the
previous valid file intact. Invalid, oversized, duplicate, or unknown-version data must not be
silently normalized into a destructive rewrite. The Windows candidate emits generic debug-output
messages for unusable state without logging player or ship identifiers.

The file must be scoped to the active save profile as well as the ship. The runtime candidate uses
the Steam account, save-slot index, and game mode, but these bindings are not yet validated. Prove
the profile identity before enabling this scope; if it cannot be resolved safely, decline to apply
state rather than risk mixing ships from separate saves.

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
   mode only after the component is ready. The current exact-build candidate writes the observed
   mode field and its associated transition timer; this has not been shown equivalent to a game
   setter and is not enabled. Avoid reapplying unchanged state or intercepting unrelated ship
   classes, AI ships, menus, or multiplayer-owned entities.

The integration must be version-gated before installing any hook. Each consumed address, signature,
structure field, and calling convention needs exact-build evidence and a fail-closed test. Hooks
must have bounded lifetime, safe teardown, and no disk access in per-frame callbacks. The current
Windows scaffold verifies the executable hash and candidate code bytes, but activation remains
explicitly disabled pending identity and end-to-end validation.

## Identity contract

Current serialized templates expose a `GcPlayerOwnershipData.Resource` value containing resource
filename, procedural seed, and alternate ID fields, separate from its `Name` field, and expose the
player's owned/current ships.
The runtime candidate derives a content key from those resource fields, canonicalizing the seed
when `UseSeedValue` is false; display names and array indices are excluded. This key is not yet a
proven unique runtime identity. Before enabling it, establish stability across rename, summon,
system travel, reload, and ownership-array reorder, and determine whether legitimate ships can
share the candidate key. Collisions or malformed resource data must fail closed.

If identity collisions are possible, identify a stronger stable field or fail closed for collisions.
Never fall back to ship names, array positions alone, model appearance, or a guessed pointer
lifetime.

## Compatibility and release gates

- A data-only EXML/MBIN solution is insufficient unless new evidence demonstrates a vanilla data
  mechanism that both observes player changes and stores independent per-ship state outside saves.
- Runtime code is optional only if the data-only alternative proves all product acceptance cases.
  Otherwise, ship a minimal exact-build-gated runtime component; do not claim broad game-version
  support.
- The current local executable is NMS 7.13, public Steam build 26385139. Static analysis maps
  power-field values 0 through 3 to Balanced, Guns, Engine, and Shield; the input action still
  stores the mode at component offset `0x675c` and its timer at `0x6760`. Candidate RVAs and the
  executable identity are build-specific and changed since 7.11. These observations do not validate
  a restore write or runtime behavior; no game build is supported.
- The executable's `GetShipComponent` call path uses the ownership object as `this` and indexes its
  component handles; this resolves the conflicting player-state annotation in the research metadata.
- A release requires unit/integration coverage for the sidecar and transition model, Windows target
  build and loader validation, a static executable-binding audit for each supported build, and
  controlled in-game checks of change/restore/rename/switch/summon/travel/reload behavior.
- No game save is modified by the implementation or its test harness. Any live behavior test uses a
  disposable save under a controlled environment.
