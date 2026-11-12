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

# Saved by Bram from the agents' `scripts/build.sh` runs. Replaces:
#   scripts/build.sh  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -1  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -2  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -3  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -4  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -5  ->  make build-debug
#   scripts/build.sh 2>&1 | tail -1 | cut -c1-80  ->  make build-debug
# Not a task: bash -n scripts/build.sh (checks the script's own syntax)
.PHONY: build-debug

# The debug build, logged to build-log.jsonl (never --release).
build-debug:
	scripts/build.sh

# Saved by Bram from the agents' `scripts/demo-instance.sh` runs. Replaces:
#   scripts/demo-instance.sh stop 2>&1 | tail -1  ->  make demo-stop
#   scripts/demo-instance.sh launch 2>&1 | tail -1  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -2  ->  make demo-launch
#   scripts/demo-instance.sh stop 2>&1 | tail -2  ->  make demo-stop
#   scripts/demo-instance.sh launch 2>&1 | tail -3  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -5  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -4  ->  make demo-launch
#   scripts/demo-instance.sh reset 2>&1 | tail -1  ->  make demo-reset
.PHONY: demo-launch demo-stop demo-reset

# Launch the default demo instance.
demo-launch:
	scripts/demo-instance.sh launch

# Stop only the default demo's recorded process.
demo-stop:
	scripts/demo-instance.sh stop

# Restore the active saved scenario, replacing live demo changes.
demo-reset:
	scripts/demo-instance.sh reset

# Saved by Bram from the agents' `cargo fmt` runs. Replaces:
#   cargo fmt  ->  make rust-fmt
#   cargo fmt --manifest-path src-tauri/Cargo.toml  ->  make rust-fmt
#   cargo fmt --check  ->  make rust-fmt-check
#   cargo fmt --manifest-path src-tauri/Cargo.toml --check  ->  make rust-fmt-check
#   cargo fmt --check >/dev/null 2>&1  ->  make rust-fmt-check
#   cargo fmt --manifest-path src-tauri/Cargo.toml --check >/dev/null 2>&1  ->  make rust-fmt-check
#   cargo fmt --check >/dev/null  ->  make rust-fmt-check
#   cargo fmt --manifest-path src-tauri/Cargo.toml --check >/dev/null  ->  make rust-fmt-check
.PHONY: rust-fmt rust-fmt-check

# Format the Rust in place. Run before committing.
rust-fmt:
	cargo fmt --manifest-path src-tauri/Cargo.toml

# Check formatting without changing anything: what the commit gate and CI run.
rust-fmt-check:
	cargo fmt --manifest-path src-tauri/Cargo.toml --check
