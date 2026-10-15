# track-rs

### Old school audio tracker for the terminal

This project is mostly intended as a fidget and a tribute to retro digital
audio. Sorry Mario, your blazingly fast modern tracker is in another repo.

### Controls

Press `?` for the current keybindings, including aliases. Press `?` again or
`Esc` to close help; `Esc` outside help does not quit. Use `Up`/`Down` to scroll
help on smaller terminals. `q` or `Q` quits, including while help is open.
Help blocks editing and playback toggles, but already-running playback continues.

### Dev

With `devenv`, `direnv`, and `nix` installed:

```bash
cd track-rs
direnv allow
cargo run
```
