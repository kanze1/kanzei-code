# Contributing

Use a `kanzei/` branch and open a pull request for a coherent change. Explain the
trigger, resulting behavior and relevant validation. Separate independent concerns;
a crate migration can include all callers needed to keep the build coherent.
Do not combine unrelated work or regenerate unrelated files.

Install frontend development dependencies with `npm ci`. Rust and native ESM are
the production stacks; React is a development-only fixture for controlled-input
browser compatibility tests.

Run `scripts/verify.ps1` during development. Its registry selects affected checks and
records each result. Run `scripts/verify.ps1 -Full` from the final clean commit before
packaging; release evidence must match that commit. Verification and release gates
run locally. GitHub Actions CI is disabled by the owner's instruction; do not add
workflows or enable remote CI for pull requests, merges or branch pushes.

Register one gate per independently scheduled test scope. Related small cases belong
under scripts/tests/ and share the domain entry's fixture lifecycle. Keep assertion
results and failures visible; do not nest unrelated suites in the runtime smoke test.
Changes to a child module must select its containing gate, including side-effect imports.

ui_browser_contracts runs the content and interaction contracts with one browser and
preview server and fresh pages for every domain. ui_state_ownership runs the seven
VM test files for prefs/stream state, session identity, surfaces, permissions, run control
and scheduling. The design audit's browser regressions use ui_design_audit_repairs.
Metrics rejection cases run with the real metrics gate; package tag cases run with
verify_policy's Windows runner contracts. Keep native desktop, real backend, platform
and model tests distinct from simulated IPC tests.

Tests that need optional external programs must use a reasoned `#[ignore]`, assert
their prerequisites when invoked, and be run explicitly in the appropriate environment
with `cargo test -p <crate> <test-filter> -- --ignored`. Report their results separately
from the default suite. Use `kanzei_base::test_support::TestDir` for owned directories
and `isolated_test`/`in_subprocess` for environment-dependent cases. Do not assert
libtest's human-readable output or serialize writers around process-global variables.

Current architecture starts at [docs/current-state.md](docs/current-state.md) and
[docs/architecture](docs/architecture). Dated reports and the October 3 history
cleanup report describe observations at their stated dates. Preserve those records;
they do not prove the current build, test coverage or release state. Refresh measurements
from code and evidence instead of appending release narratives to baselines.

The pre-October-3 history is retained in the external bundle documented in
[docs/cleanup-2026-10-03.md](docs/cleanup-2026-10-03.md). Recover it in an independent
checkout when needed; do not splice a replacement history into a shared branch.
