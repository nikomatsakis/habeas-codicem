# Test coverage

Collect Rust unit and integration test coverage with `habeas coverage`.
For focused runs, forward nextest arguments with `habeas coverage -- -E 'test(name)'`.
Run doctests separately with `cargo test --doc --workspace`.

Coverage is disposable build output in the Cargo target directory. The index
combines each test's latest compatible attempt; source changes invalidate its
contribution. Building this book reads saved reports and never runs tests.

The overview lists files in compact tables. Expand a directory to browse its
files; its summary shows the lowest measured file coverage, including nested
directories, and counts any unmeasured or unavailable files. Choose a test to
see only the files it executes, including its child processes.

Click a Rust file to see its current `habeas:coverage` explanations and citation
review states. A file below 80% needs a file-root explanation; each wholly
unexecuted or unmeasured function needs its own explanation. Directory summaries
add no exception obligations. Notes always describe current workspace source,
even when opened from a historical test report.

Expand the optional details to inspect run metadata or individual source lines.

:::{coverage-index}
:::
