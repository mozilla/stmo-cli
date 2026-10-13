use clap::{Arg, ArgAction, Command};
use std::fmt::Write;

#[must_use]
pub fn render(command: &Command) -> String {
    let mut out = String::new();
    render_command(&mut out, command.get_name(), command);
    out
}

fn render_command(out: &mut String, path: &str, command: &Command) {
    let subcommands: Vec<&Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .collect();

    if subcommands.is_empty() {
        render_leaf(out, path, command);
        return;
    }

    for sub in subcommands {
        render_command(out, &format!("{path} {}", sub.get_name()), sub);
    }
}

fn render_leaf(out: &mut String, path: &str, command: &Command) {
    let _ = writeln!(out, "## {path}");
    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        let _ = writeln!(out, "{about}");
    }
    for arg in visible_args(command) {
        let _ = writeln!(out, "- {}", describe_arg(arg));
    }
    out.push('\n');
}

fn visible_args(command: &Command) -> impl Iterator<Item = &Arg> {
    command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && !matches!(arg.get_id().as_str(), "help" | "version"))
}

fn describe_arg(arg: &Arg) -> String {
    let mut line = signature(arg);
    if let Some(help) = arg.get_help() {
        let _ = write!(line, ": {help}");
    }
    if let Some(default) = default_value(arg) {
        let _ = write!(line, " (default: {default})");
    }
    line
}

fn signature(arg: &Arg) -> String {
    if arg.is_positional() {
        return positional_signature(arg);
    }
    option_signature(arg)
}

fn positional_signature(arg: &Arg) -> String {
    let name = value_name(arg);
    let ellipsis = if is_repeatable(arg) { "..." } else { "" };
    format!("[{name}]{ellipsis}")
}

fn option_signature(arg: &Arg) -> String {
    let mut flags = Vec::new();
    if let Some(long) = arg.get_long() {
        flags.push(format!("--{long}"));
    }
    if let Some(short) = arg.get_short() {
        flags.push(format!("-{short}"));
    }
    let mut signature = flags.join(", ");
    if arg.get_action().takes_values() {
        let _ = write!(signature, " <{}>", value_name(arg));
    }
    signature
}

fn value_name(arg: &Arg) -> String {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map_or_else(|| arg.get_id().as_str().to_uppercase(), ToString::to_string)
}

fn is_repeatable(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append)
}

fn default_value(arg: &Arg) -> Option<String> {
    let values = arg.get_default_values();
    let first = values.first()?;
    Some(first.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Command {
        Command::new("stmo-cli")
            .subcommand(
                Command::new("fetch")
                    .about("Fetch queries from Redash")
                    .arg(
                        Arg::new("query_ids")
                            .action(ArgAction::Append)
                            .help("Query IDs to fetch"),
                    )
                    .arg(
                        Arg::new("all")
                            .long("all")
                            .action(ArgAction::SetTrue)
                            .help("Fetch all tracked queries"),
                    ),
            )
            .subcommand(
                Command::new("discover")
                    .about("List queries")
                    .arg(
                        Arg::new("search")
                            .long("search")
                            .short('q')
                            .value_name("TEXT")
                            .help("Search by text"),
                    )
                    .arg(
                        Arg::new("limit")
                            .long("limit")
                            .default_value("50")
                            .help("Max results"),
                    ),
            )
            .subcommand(
                Command::new("dashboards")
                    .subcommand(Command::new("discover").about("List dashboards")),
            )
            .subcommand(Command::new("secret").hide(true).about("Hidden"))
    }

    #[test]
    fn renders_every_visible_leaf_command_with_full_path() {
        let rendered = render(&fixture());

        assert!(rendered.contains("## stmo-cli fetch\n"));
        assert!(rendered.contains("## stmo-cli discover\n"));
        assert!(rendered.contains("## stmo-cli dashboards discover\n"));
    }

    #[test]
    fn skips_hidden_and_help_commands() {
        let rendered = render(&fixture());

        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("help"));
    }

    #[test]
    fn renders_about_text_under_heading() {
        let rendered = render(&fixture());

        assert!(rendered.contains("## stmo-cli fetch\nFetch queries from Redash\n"));
    }

    #[test]
    fn renders_option_with_short_flag_and_value_name() {
        let rendered = render(&fixture());

        assert!(rendered.contains("- --search, -q <TEXT>: Search by text"));
    }

    #[test]
    fn renders_flag_without_value() {
        let rendered = render(&fixture());

        assert!(rendered.contains("- --all: Fetch all tracked queries"));
    }

    #[test]
    fn renders_repeatable_positional_with_ellipsis() {
        let rendered = render(&fixture());

        assert!(rendered.contains("- [QUERY_IDS]...: Query IDs to fetch"));
    }

    #[test]
    fn renders_default_value() {
        let rendered = render(&fixture());

        assert!(rendered.contains("- --limit <LIMIT>: Max results (default: 50)"));
    }
}
