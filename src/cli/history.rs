use crate::infra::history::{export_history_recap, parse_recap_period};
use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use std::path::PathBuf;

pub fn history_subcommand() -> Command {
  Command::new("history")
    .about("Work with echovena local listening history")
    .subcommand_required(true)
    .subcommand(
      Command::new("recap")
        .about("Generate a shareable HTML recap from local listening history")
        .arg(
          Arg::new("period")
            .long("period")
            .default_value("30d")
            .value_parser(["7d", "30d", "month", "year", "all"])
            .help("Time range to include in the recap"),
        )
        .arg(
          Arg::new("output")
            .long("output")
            .short('o')
            .default_value("echovena-recap.html")
            .value_name("PATH")
            .help("Path to write the HTML recap file"),
        ),
    )
}

pub fn handle_history_matches(matches: &ArgMatches) -> Result<String> {
  let recap_matches = matches
    .subcommand_matches("recap")
    .expect("clap guarantees a history subcommand");
  let period = parse_recap_period(
    recap_matches
      .get_one::<String>("period")
      .expect("period has default"),
  )?;
  let output_path = PathBuf::from(
    recap_matches
      .get_one::<String>("output")
      .expect("output is required"),
  );
  let listen_count = export_history_recap(period, &output_path)?;
  Ok(format!(
    "Generated recap from {} qualified listens at {}",
    listen_count,
    output_path.display()
  ))
}
