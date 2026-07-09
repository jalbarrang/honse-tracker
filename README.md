# honse-tracker

Standalone plugin workspace targeting upstream **hachimi-edge**.

Full docs land in plan 5. Living SDK example: `cargo build --release -p debug-viewer`.

## Intent tests

Before `cargo test`, regenerate the hiker property tests:

```
hiker gen .hiker/tents/honse-extraction/honse-extraction.tent --target rust --module honse_tracker
```
