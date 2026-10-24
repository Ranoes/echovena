//! The `echovena mcp` subcommand.
//!
//! Spawned by an MCP client (`claude mcp add echovena -- echovena mcp`), it
//! speaks MCP on stdio and relays to the running TUI. See
//! [`crate::infra::mcp`] for the architecture.

use clap::{Arg, ArgAction, Command};

pub fn mcp_subcommand() -> Command {
  Command::new("mcp")
    // Deliberately NOT `subcommand_required`: bare `echovena mcp` is the server
    // itself, and that is what clients are registered to run.
    .subcommand(
      Command::new("status")
        .about("Check whether the MCP setup is working (safe to run; does not block)")
        .long_about(
          "Verifies each step of the MCP setup and exits non-zero if it is not \
           ready.\n\n\
           Run this instead of `echovena mcp` when testing: `echovena mcp` IS the \
           server, so it reads JSON-RPC from stdin and blocks until closed.",
        )
        .arg(
          Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help("Emit machine-readable JSON"),
        ),
    )
    .about("Run as an MCP server so a coding agent can drive echovena")
    .long_about(
      "Serves the Model Context Protocol on stdin/stdout so Claude Code, Codex, \
       Gemini CLI, or any MCP client can control playback and read your listening \
       history.\n\n\
       Register it once with your client, e.g.:\n\
       \x20 claude mcp add echovena -- echovena mcp\n\
       \x20 codex mcp add echovena -- echovena mcp\n\n\
       The running echovena instance does the work, so start echovena and set \
       `behavior.mcp_enabled: true` in its config. Without a running instance the \
       server still starts, but every tool reports that the player is \
       unavailable.",
    )
}
