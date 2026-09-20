# Notices

The native frontend and tray lifecycle are adapted from the AIU Rust desktop
frontend (`krflol/aiu-rs`), released under the MIT License. The adapter keeps
credentials, provider requests, storage, recommendation ranking, and account
selection in the sibling Go executable; this crate only renders its JSONL
frontend protocol and forwards user actions.
