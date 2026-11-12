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
# Saved by Bram from the agents' `cargo build` runs. Replaces:
#   cargo build  ->  make build-debug
#   cargo build --manifest-path src-tauri/Cargo.toml  ->  make build-debug
#   cargo build --manifest-path src-tauriCargo.toml --target x86_64-pc-windows-msvc  ->  make build-debug
#   cargo build --manifest-path src-tauri\Cargo.toml --target x86_64-pc-windows-msvc --offline  ->  make build-debug
#   cargo build --manifest-path src-tauri\Cargo.toml --target x86_64-pc-windows-msvc --target-dir "$targetDir"  ->  make build-debug
# Not a task: cargo build --manifest-path src-tauri/Cargo.toml --lib (compiles the library alone, a one-off check)
# Not a task: cargo build --manifest-path C:\Users\jon\AppData\Local\Temp\bram-clear-probe-project\Cargo.toml --target x86_64-pc-windows-msvc --target-dir .\src-tauri\target --offline (a throwaway probe project)
# Saved by Bram from the agents' `scripts/build.ps1` runs. Replaces:
#   powershell -NoProfile -File scripts/build.ps1 2>&1 | tail -4  ->  make build-debug
#   powershell -NoProfile -File scripts/build.ps1 2>&1 | tail -15  ->  make build-debug
#   powershell -NoProfile -File scripts/build.ps1 2>&1 | tail -3  ->  make build-debug
#   powershell -NoProfile -File scripts/build.ps1 2>&1 | tail -6  ->  make build-debug
.PHONY: build-debug

# The debug build, logged to build-log.jsonl (never --release).
ifeq ($(OS),Windows_NT)
build-debug:
	powershell -NoProfile -File scripts\build.ps1
else
build-debug:
	scripts/build.sh
endif

# Saved by Bram from the agents' `scripts/demo-instance.sh` runs. Replaces:
#   scripts/demo-instance.sh stop 2>&1 | tail -1  ->  make demo-stop
#   scripts/demo-instance.sh launch 2>&1 | tail -1  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -2  ->  make demo-launch
#   scripts/demo-instance.sh stop 2>&1 | tail -2  ->  make demo-stop
#   scripts/demo-instance.sh launch 2>&1 | tail -3  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -5  ->  make demo-launch
#   scripts/demo-instance.sh launch 2>&1 | tail -4  ->  make demo-launch
#   scripts/demo-instance.sh reset 2>&1 | tail -1  ->  make demo-reset
# Saved by Bram from the agents' `scripts/demo-instance.ps1` runs. Replaces:
#   powershell -NoProfile -File scripts/demo-instance.ps1 stop 2>&1 | tail -1  ->  make demo-stop
#   powershell -NoProfile -File scripts/demo-instance.ps1 launch 2>&1 | tail -1  ->  make demo-launch
#   powershell -NoProfile -File scripts/demo-instance.ps1 launch 2>&1 | tail -2  ->  make demo-launch
#   powershell -NoProfile -File scripts/demo-instance.ps1 launch 2>&1 | tail -10  ->  make demo-launch
#   powershell -NoProfile -File scripts/demo-instance.ps1 reset 2>&1 | tail -1  ->  make demo-reset
#   powershell -NoProfile -File scripts/demo-instance.ps1 use gate-check --legacy-layout 2>&1 | tail -1  ->  make demo-use S=gate-check LEGACY=1
#   powershell -NoProfile -File scripts/demo-instance.ps1 use gate-check --legacy-layout 2>&1 | tail -4  ->  make demo-use S=gate-check LEGACY=1
# Not a task: powershell -NoProfile -File scripts/demo-instance.ps1 status 2>&1 | grep -E 'pid' (reads the demo's PID)
.PHONY: demo-launch demo-stop demo-reset demo-use

# Launch the default demo instance.
ifeq ($(OS),Windows_NT)
demo-launch:
	powershell -NoProfile -File scripts\demo-instance.ps1 launch
else
demo-launch:
	scripts/demo-instance.sh launch
endif

# Stop only the default demo's recorded process.
ifeq ($(OS),Windows_NT)
demo-stop:
	powershell -NoProfile -File scripts\demo-instance.ps1 stop
else
demo-stop:
	scripts/demo-instance.sh stop
endif

# Restore the active saved scenario, replacing live demo changes.
ifeq ($(OS),Windows_NT)
demo-reset:
	powershell -NoProfile -File scripts\demo-instance.ps1 reset
else
demo-reset:
	scripts/demo-instance.sh reset
endif

# Put the demo on a saved scenario. Usage: make demo-use S=<scenario> [LEGACY=1]
# LEGACY=1 hydrates the old resources/ layout, to exercise the startup move.
ifeq ($(OS),Windows_NT)
demo-use:
	powershell -NoProfile -File scripts\demo-instance.ps1 use $(S) $(if $(LEGACY),--legacy-layout)
else
demo-use:
	scripts/demo-instance.sh use $(S) $(if $(LEGACY),--legacy-layout)
endif

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

# Saved by Bram from the agents' `scripts/switch-matrix.py` runs. Replaces:
#   PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py --no-turns --agents claude,codex,opencode > "$S/matrix-noturns-wave2.txt" 2>&1  ->  make switch-matrix AGENTS=claude,codex,opencode NO_TURNS=1
#   PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py --agents claude,opencode > "C:/Users/jon/AppData/Local/Temp/claude/C--Users-jon-bram/ffba9ce7-4ce8-47fc-a4dd-9734ecd62951/scratchpad/matrix2.txt" 2>&1  ->  make switch-matrix AGENTS=claude,opencode
#   PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py --no-turns --agents claude,codex,opencode > "$S/matrix-noturns-wave2-retry.txt" 2>&1  ->  make switch-matrix AGENTS=claude,codex,opencode NO_TURNS=1
#   PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py 2>&1 | tee "C:/Users/jon/AppData/Local/Temp/claude/C--Users-jon-bram/ffba9ce7-4ce8-47fc-a4dd-9734ecd62951/scratchpad/matrix.txt"  ->  make switch-matrix
#   do PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py --agents claude,opencode --new-session-claude-only > "$S/ns$i.txt" 2>&1  ->  make switch-matrix AGENTS=claude,opencode FLAGS=--new-session-claude-only
#   do PYTHONIOENCODING=utf-8 python -I scripts/switch-matrix.py --agents claude,opencode > "$S/race$i.txt" 2>&1  ->  make switch-matrix AGENTS=claude,opencode
#   python -I scripts/switch-matrix.py --agents claude --new-session-claude-only --keep 2>&1 | tail -8  ->  make switch-matrix AGENTS=claude FLAGS="--new-session-claude-only --keep"
#   python -I scripts/switch-matrix.py --agents claude,codex 2>&1 | tail -60  ->  make switch-matrix AGENTS=claude,codex
.PHONY: switch-matrix

# The session-switching matrix against a demo instance, with the real agent
# CLIs. Usage: make switch-matrix [AGENTS=claude,codex] [NO_TURNS=1] [FLAGS=...]
# NO_TURNS=1 skips the cases that start a model turn. FLAGS passes the rest
# (--new-session-claude-only, --keep). -X utf8 replaces PYTHONIOENCODING.
ifeq ($(OS),Windows_NT)
switch-matrix:
	python -X utf8 -I scripts/switch-matrix.py $(if $(AGENTS),--agents $(AGENTS)) $(if $(NO_TURNS),--no-turns) $(FLAGS)
else
switch-matrix:
	python3 -X utf8 -I scripts/switch-matrix.py $(if $(AGENTS),--agents $(AGENTS)) $(if $(NO_TURNS),--no-turns) $(FLAGS)
endif

# Saved by Bram from the agents' `curl` runs. Replaces:
# Not a task: curl -4 -sS -o /dev/null -w 'site: %{http_code}\n' https://judell.github.io/bram-tasks-design/ (a one-off check that a page was up)
# Not a task: curl …/__worklist/commit' command claims (a text fragment, not a command)
# Not a task: c=$(curl -s -m 60 -o $S/probe.txt -w '%{http_code}' 'https://web.archive.org/cdx/search/cdx?url=educause.edu&limit=1') (a one-off probe)
# Not a task: curl --retry-connrefused --retry 3 --retry-delay 1 -s https://api.github.com/repos/judell/bram/issues/103 (one issue fetch; gh does this)
