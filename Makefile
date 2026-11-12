# Saved by Bram from the agents' `cargo test` runs. Replaces:
#   cargo test  ->  make rust-tests-all
#   cargo test --manifest-path src-tauri/Cargo.toml  ->  make rust-tests-all
#   cargo test pty_menu_tests  ->  make rust-tests-module M=pty_menu_tests
#   cargo test agent_status_tests  ->  make rust-tests-module M=agent_status_tests
#   cargo test turn_completion_tests  ->  make rust-tests-module M=turn_completion_tests
#   cargo test worklist_history_tests  ->  make rust-tests-module M=worklist_history_tests
#   cargo test --manifest-path src-tauri/Cargo.toml send_ledger  ->  make rust-tests-module M=send_ledger
# Not a task: cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | grep "test result" | head -1 (reads only the summary line)
.PHONY: rust-tests-all rust-tests-module

# Every Rust test, from the repo root.
rust-tests-all:
	cargo test --manifest-path src-tauri/Cargo.toml

# One test module. Usage: make rust-tests-module M=pty_menu_tests
rust-tests-module:
	cargo test --manifest-path src-tauri/Cargo.toml $(M)
