# CI coverage checks

{impl}`requirements.test-coverage.check.guidance`

Install the Habeas CLI version matching the project's dependency,
`cargo-nextest` (>= 0.9.116), and the active Rust toolchain's LLVM tools
(`rustup component add llvm-tools-preview`) in CI.

Add these steps to the project's test job:

```yaml
steps:
  - name: Collect workspace execution coverage
    run: habeas coverage -- --locked --workspace
  - name: Enforce execution coverage
    run: habeas coverage-check
  - name: Run doctests
    run: cargo test --locked --doc --workspace
```

Keep collection and checking in the same job, checkout, and Cargo target
directory. Use the steps' normal success conditions so the check runs only after
successful collection and a failed check fails the job. Collection alone does
not enforce coverage.

`habeas test` runs full-workspace collection and then the check locally.
The generated `tests/habeas.rs` validates saved requirement evidence and collector
configuration; it does not enforce execution coverage and need not run last.
`habeas setup` configures that test and the collector. Add these CI steps to
the project's workflow separately.

Checking requires 80% instrumented-line coverage per first-party Rust file and
execution or a current accepted explanation for every function. The Habeas
skill explains standard `habeas:coverage` citations. Author their independent
reviews locally with `habeas author` and commit the evidence; checking in CI
runs no model. Keep disposable coverage artifacts out of Git.

This repository uses `cargo habeas` instead of the installed `habeas` CLI.
