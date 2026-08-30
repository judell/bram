// Dedicated guard entrypoint (windows-guard-bin-no-console): the same
// crate and the same run_guard_mode as `bram guard …`, but carrying
// windows_subsystem = "windows" UNCONDITIONALLY — debug is the shipping
// format, and the console-subsystem main binary allocates a conhost per
// hook spawn (the flash class confirmed structural in #297/#313:
// `windows_subsystem` is build-wide, so guard and app could not differ
// while they were one executable). GUI-subsystem stdio over pipes is how
// hooks talk; the Windows smoke on #313 is the live proof.
//
// Registration strings keep the historical `guard <hook> [--authority]`
// shape so settings.json and config.toml need no migration — only the
// artifact behind ~/.bram/bram-guard changes — which is why a leading
// literal `guard` is stripped here.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("guard") {
        args.remove(0);
    }
    std::process::exit(bram_lib::guard::run_guard_mode(&args));
}
