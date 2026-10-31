# SPSP repository rules

## Product acceptance

- Deliver per-ship Starship Power Diversion persistence without reading from or writing to any NMS
  save file. Store only the minimum required state in SPSP's own sidecar.
- Key settings by a verified, name-independent ship identity. Prove stability across ship renames,
  save/reload, ship summon/switch, and system travel. Handle duplicate identities and multiple save
  profiles without applying one ship's setting to another; ambiguous identity must fail closed.
- Capture an explicit player-selected mode before the game can reset it, and restore it only after
  the corresponding owned ship is positively identified and ready. Never infer a selection from a
  transient default or an AI/non-player ship.
- Prefer the narrowest game-supported mechanism. The current data-mod loader cannot execute new
  behavior, so any required runtime integration must be exact-build-qualified and narrowly scoped.
  Do not ship guessed offsets, permissive signature matches, or broad memory scans as a supported
  implementation.
- No I/O or polling on a per-frame hot path. Persist only on a real setting change, use a bounded
  versioned file, and make replacement atomic so interrupted writes preserve the last valid state.
  Corrupt or future-version state must be reported and preserved for recovery, not silently erased.
- A release must include deterministic tests for identity, mode mapping, serialization, corruption,
  atomic replacement, state transitions, unsupported builds, duplicate identities, and unrelated
  ship handling. Static checks do not substitute for controlled in-game validation.
- Keep the user interface and install footprint minimal. Do not add a UI, settings system, extra
  dependencies, or compatibility shims unless evidence shows they are necessary.

## Mandatory engineering policy

- Run and pass the complete configured pre-commit suite before every commit. Do not skip hooks,
  selectively disable checks, or use `--no-verify`. CI must run the same configured checks and block
  merging when any fail.
- Inline lint, formatter, test, and security suppressions are prohibited. Do not add `allow`, `expect`,
  `noqa`, `type: ignore`, `nosec`, formatter-ignore, or equivalent suppression directives. Fix the
  underlying issue or improve a check globally with a reviewed rationale and regression test.
- Every third-party GitHub Action and direct application dependency must be pinned to an exact
  immutable revision/version. Commit `Cargo.lock`; all Cargo commands that resolve dependencies use
  `--locked`. Security updates arrive as reviewable pull requests and are never auto-merged.
- Keep Harden-Runner and least-privilege workflow permissions enabled. Any egress-policy change must
  be justified by the workflow's actual required endpoints.
- Do not commit game assets, save files, extracted third-party content, user sidecars, binaries,
  investigation runs, or generated build output.
- Keep documentation current and concise. Do not retain investigation transcripts, run IDs, hashes,
  or obsolete implementation paths as product documentation.

## Investigation and test safety

- Follow the parent workspace's No Man's Sky guide and investigation-tooling gates before making
  game-specific claims. Put task-local input and output in the selected Squinch tranche; remove it
  when complete.
- Keep runtime hooks disabled unless the exact game build and all bindings are validated. Never
  patch or write a live save to test SPSP. Do not automate gameplay against a personal save. Use a
  disposable save and a controlled, user-authorized test setup for runtime validation.
- Preserve unrelated parent-workspace changes and all other mod submodules. Work only in this
  repository unless explicit parent-workspace registration is required.
