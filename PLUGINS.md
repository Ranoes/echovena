# Plugins

EchoVena runs user-written Lua plugins. They react to playback events, add commands and key
bindings, draw popups and playbar segments, restyle the theme, and make async HTTP requests.
See [`docs/scripting.md`](docs/scripting.md) for the full API and [`examples/plugins/`](examples/plugins) for runnable examples.

## Installing a plugin

Plugins published as git repositories install with one command (requires `git`):

```bash
echovena plugin add owner/repo     # clone + record in the lockfile
echovena plugin list               # show installed plugins
echovena plugin update             # update all to their latest commit
echovena plugin remove <name>      # uninstall
echovena plugin new <name>         # scaffold a new plugin to start from
```

Plugins are cloned into `plugins/<name>/` under the EchoVena app config directory
(`$XDG_CONFIG_HOME/echovena` or `~/.config/echovena`) and loaded at startup.
Restart EchoVena after installing, and bind any commands the plugin registers
under `plugin_commands` in `config.yml`.

Plugins are not sandboxed and run with full app privileges and network access, so only install
ones you trust. See [Trust and safety](docs/scripting.md#trust-and-safety).

You can also drop a single `.lua` file into the app config directory's `plugins/` folder by hand.

## First-party examples

Runnable examples are in [`examples/plugins/`](examples/plugins):

- `now-playing.lua`: notification popup on track change
- `session-stats/`: cumulative listening duration & track counter
- `track-info-popup.lua`: detailed metadata inspect popup
