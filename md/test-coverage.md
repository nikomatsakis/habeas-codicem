# Test coverage

Collect Rust unit and integration test coverage with `habeas coverage`.
For focused runs, forward nextest arguments with `habeas coverage -- -E 'test(name)'`.
Run doctests separately with `cargo test --doc --workspace`.

Coverage is disposable build output in the Cargo target directory. The index
combines each test's latest compatible attempt; source changes invalidate its
contribution. Building this book reads saved reports and never runs tests.

The overview maps project files to their line coverage percentages. Choose a
test to see only the files it executes, including its child processes. Expand
the optional details to inspect run metadata or individual source lines.

:::{coverage-index}
:::
