This is the user manual for the XMLUI surface that lives in the right pane
of *xmlui-desktop*. (The dialog's title bar already says
&ldquo;User manual&rdquo; &mdash; no in-body H1 needed.)

## What you're looking at

The window has two panes:

- **Left pane**: a real terminal (xterm.js + a Rust PTY child) where you
  can run `claude`, `bash`, or anything else.
- **Right pane**: this XMLUI surface. Its content lives at
  `~/xmlui-desktop/Main.xmlui` (with components under `components/`).
  Edit those files from the left pane (or anywhere) and the right
  pane reloads automatically.

## Closing the loop

When something in the right pane reports an event back to the parent
shell, that event can either:

1. Show up in the host process's stderr (the terminal where `cargo run`
   is running), prefixed `[right-pane]`, or
2. Be injected into the PTY as user input, where the foreground process
   (often `claude`) reads it as a normal user message.

This is the start of the &ldquo;Claude in the shell, XMLUI as the surface&rdquo;
loop. Selections, button clicks, and form submissions all become input
Claude can react to.

## Editing this manual

This document lives at `manual.md` at the project root. Edit it the same way you
edit `Main.xmlui` &mdash; changes auto-reload thanks to the filesystem
watcher in the Rust shell.

## Keyboard shortcuts

*(none yet &mdash; we'll add cmd-key passthrough, copy-on-selection,
bracketed paste, and friends as the terminal side gets polish.)*

## Architecture

*(the project doc at `~/.agents/scout/projects/claude-code-desktop.md`
has the deep version &mdash; this section will summarize the bits a user,
not a developer, needs to know.)*
