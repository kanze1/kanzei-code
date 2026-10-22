# Engineering rules

- Complete authorized work and resolve routine choices from repository context.
- Inspect ownership, callers, data flow and existing abstractions before editing.
- Repair violated invariants and remove obsolete paths; keep one implementation per contract.
- Preserve unrelated changes. Keep each commit focused on one reviewable concern.
- Do not hide failures with broad catches, silent defaults, retries or test self-skips.
- Depend directly on the crate that owns a capability. `kanzei-runtime` composes tools,
  memory, core and harness; capability crates must never depend on that composition root.
- `kanzei-core::store` is private. Use the public domain services and typed facts.
- Environment-mutating tests run in a child process with a completion receipt.
  A mutex around writers does not isolate concurrent readers.
- External prerequisites belong in explicitly ignored tests with a reason. Running
  those tests explicitly must fail when a prerequisite is missing.
- Reproduce the original failure, exercise the relevant tests and verify the invariant.
  Run `scripts/verify.ps1` for the affected checks and `scripts/verify.ps1 -Full` on
  the final clean commit before release. Passing, ignored and unexecuted are distinct.
- Never rewrite published history, delete historical refs or reset a shared branch
  as routine cleanup. Preserve history and fix forward.

See [CONTRIBUTING.md](CONTRIBUTING.md) for verification and review workflow.
