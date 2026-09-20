use crate::error::{Error, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub subcommand: Subcommand,
    /// Effective color mode (CLI `--color` overrides `pagina.toml`).
    pub color: ColorMode,
}

#[derive(Debug, Clone)]
pub enum Subcommand {
    ToHtml {
        input: String,
        output: String,
        gfm: bool,
    },
    ToMd {
        input: String,
        output: String,
        gfm: bool,
    },
    Help,
    Version,
}

// ---------------------------------------------------------------------------
// Color mode (`--color=auto/always/never`, `pagina.toml: color = ...`)
// ---------------------------------------------------------------------------

/// When to emit ANSI colors in help text and error messages.
///
/// Converted output (stdout file bytes) is never colorized regardless of
/// this setting; only human-facing diagnostics are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Emit colors only when the target stream is a TTY.
    #[default]
    Auto,
    /// Always emit colors, even when piped.
    Always,
    /// Never emit colors.
    Never,
}

impl FromStr for ColorMode {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" => Ok(ColorMode::Always),
            "never" => Ok(ColorMode::Never),
            other => Err(Error::InvalidInput(format!(
                "Invalid --color value '{}' (expected auto, always, or never)",
                other
            ))),
        }
    }
}

impl std::fmt::Display for ColorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorMode::Auto => write!(f, "auto"),
            ColorMode::Always => write!(f, "always"),
            ColorMode::Never => write!(f, "never"),
        }
    }
}

impl ColorMode {
    /// Resolve against a TTY probe (`true` when the target stream is a TTY).
    pub fn use_color(self, stream_is_tty: bool) -> bool {
        match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => stream_is_tty,
        }
    }
}

// Manual ANSI escapes (no `anstream` dependency; zero-dep default must stay).
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

fn paint(text: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("{}{}{}", code, text, RESET)
    } else {
        text.to_string()
    }
}

// ---------------------------------------------------------------------------
// pagina.toml config
// ---------------------------------------------------------------------------

/// File-backed defaults: `./pagina.toml` overrides
/// `~/.config/pagina/config.toml`, which overrides built-in defaults.
///
/// Recognized keys: `gfm` (bool), `color` (`"auto"`/`"always"`/`"never"`).
/// Unknown keys (and unparseable `color` values) are silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub gfm: bool,
    pub color: ColorMode,
}

impl Config {
    /// Load from disk: home config first, then `./pagina.toml` wins.
    pub fn load() -> Self {
        let mut cfg = Config::default();
        if let Some(home) = home_config_path() {
            if let Ok(text) = std::fs::read_to_string(&home) {
                cfg.apply_str(&text);
            }
        }
        let local = std::env::current_dir()
            .map(|d| d.join("pagina.toml"))
            .unwrap_or_else(|_| PathBuf::from("pagina.toml"));
        if let Ok(text) = std::fs::read_to_string(&local) {
            cfg.apply_str(&text);
        }
        cfg
    }

    /// Parse TOML-subset text (`key = value` lines; `[table]` headers and
    /// unknown keys skipped). Used by [`Config::load`] and unit tests.
    pub fn load_from_str(s: &str) -> Self {
        let mut cfg = Config::default();
        cfg.apply_str(s);
        cfg
    }

    /// Load one file, returning defaults when unreadable.
    pub fn load_from_file(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::load_from_str(&text),
            Err(_) => Self::default(),
        }
    }

    fn apply_str(&mut self, text: &str) {
        for raw_line in text.lines() {
            let line = strip_config_comment(raw_line).trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            // `[table]` headers: outside the two-key subset, skip.
            if line.starts_with('[') {
                continue;
            }
            let Some(eq) = line.find('=') else {
                continue;
            };
            let key = line[..eq].trim().to_ascii_lowercase();
            let value = unquote_config_value(line[eq + 1..].trim());
            match key.as_str() {
                "gfm" => {
                    if let Some(b) = parse_config_bool(&value) {
                        self.gfm = b;
                    }
                }
                "color" => {
                    if let Ok(mode) = value.parse::<ColorMode>() {
                        self.color = mode;
                    }
                    // Unparseable colors are ignored (unknown-key tolerance).
                }
                _ => {} // Unknown keys ignored with no error.
            }
        }
    }
}

/// Candidate home config path (`~/.config/pagina/config.toml`), or `None`
/// when neither `HOME` nor `USERPROFILE` is set.
pub fn home_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".config").join("pagina").join("config.toml"))
}

/// Strip a trailing `#` comment, ignoring `#` inside quotes.
fn strip_config_comment(s: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return s[..i].trim_end(),
            _ => {}
        }
    }
    s
}

/// Remove one layer of surrounding single/double quotes.
fn unquote_config_value(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 {
        let bytes = t.as_bytes();
        let (first, last) = (bytes[0], bytes[t.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

fn parse_config_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

/// Split a `--gfm` flag out of positional args. Accepted anywhere after the
/// subcommand (`pagina to-html --gfm in out`, `pagina to-html in out --gfm`).
fn split_gfm(args: &[String]) -> (Vec<String>, bool) {
    let mut positionals = Vec::with_capacity(args.len());
    let mut gfm = false;
    for a in args {
        if a == "--gfm" {
            gfm = true;
        } else {
            positionals.push(a.clone());
        }
    }
    (positionals, gfm)
}

/// Pull `--color=<v>` / `--color <v>` out of an arg slice.
///
/// Returns the remaining args plus the last `--color` value seen. An
/// invalid or missing value is an error.
fn split_color(args: &[String]) -> Result<(Vec<String>, Option<ColorMode>)> {
    let mut rest = Vec::with_capacity(args.len());
    let mut color: Option<ColorMode> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(value) = a.strip_prefix("--color=") {
            color = Some(value.parse::<ColorMode>()?);
        } else if a == "--color" {
            i += 1;
            let Some(value) = args.get(i) else {
                return Err(Error::MissingArgument(
                    "--color requires a value (auto, always, or never)".to_string(),
                ));
            };
            color = Some(value.parse::<ColorMode>()?);
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    Ok((rest, color))
}

/// Best-effort `--color` sniff for the error path (when full parsing has
/// already failed and no `CliArgs` exists to carry the preference).
/// Never errors: unparseable values fall back to `Auto`.
pub fn sniff_color(argv: &[String]) -> ColorMode {
    for (i, a) in argv.iter().enumerate() {
        if let Some(value) = a.strip_prefix("--color=") {
            if let Ok(mode) = value.parse::<ColorMode>() {
                return mode;
            }
        } else if a == "--color" {
            if let Some(value) = argv.get(i + 1) {
                if let Ok(mode) = value.parse::<ColorMode>() {
                    return mode;
                }
            }
        }
    }
    Config::load().color
}

fn build_args(subcommand: Subcommand, color: Option<ColorMode>, config: &Config) -> CliArgs {
    CliArgs {
        subcommand,
        color: color.unwrap_or(config.color),
    }
}

/// Parse `argv` (including the program name at index 0) with an explicit
/// config. Pure except for no I/O: the workhorse for tests.
pub fn parse_args_from_with_config(argv: &[String], config: &Config) -> Result<CliArgs> {
    if argv.len() < 2 {
        return Ok(build_args(Subcommand::Help, None, config));
    }

    // `--color` may appear before the subcommand (`pagina --color=never
    // to-html …`) or after it (`pagina to-html --color=never …`).
    let (argv_no_color, global_color) = split_color(argv)?;
    // `split_color` preserves order, so index 0 is still the program name.
    let args: &[String] = if argv_no_color.is_empty() {
        &[]
    } else {
        &argv_no_color[1..]
    };

    let first = args.first().map(|s| s.as_str());
    match first {
        None => Ok(build_args(Subcommand::Help, global_color, config)),
        Some("to-html") => {
            let (positionals, cli_gfm) = split_gfm(&args[1..]);
            // `--color` after the subcommand lands in `positionals` only
            // when written as a separate token pair; re-split to catch it.
            let (positionals, local_color) = split_color(&positionals)?;
            let color = local_color.or(global_color);
            if positionals.len() < 2 {
                return Err(Error::MissingArgument(
                    "to-html requires <input.md> <output.html> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(build_args(
                Subcommand::ToHtml {
                    input: positionals[0].clone(),
                    output: positionals[1].clone(),
                    gfm: cli_gfm || config.gfm,
                },
                color,
                config,
            ))
        }
        Some("to-md") => {
            let (positionals, cli_gfm) = split_gfm(&args[1..]);
            let (positionals, local_color) = split_color(&positionals)?;
            let color = local_color.or(global_color);
            if positionals.len() < 2 {
                return Err(Error::MissingArgument(
                    "to-md requires <input.html> <output.md> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(build_args(
                Subcommand::ToMd {
                    input: positionals[0].clone(),
                    output: positionals[1].clone(),
                    gfm: cli_gfm || config.gfm,
                },
                color,
                config,
            ))
        }
        Some("--help") | Some("-h") | Some("help") => {
            Ok(build_args(Subcommand::Help, global_color, config))
        }
        Some("--version") | Some("-v") | Some("version") => {
            Ok(build_args(Subcommand::Version, global_color, config))
        }
        Some(other) => Err(Error::InvalidSubcommand(other.to_string())),
    }
}

/// Parse `argv` (including the program name) with file config loaded from
/// disk (`./pagina.toml`, then `~/.config/pagina/config.toml`).
pub fn parse_args_from(argv: &[String]) -> Result<CliArgs> {
    let config = Config::load();
    parse_args_from_with_config(argv, &config)
}

pub fn parse_args() -> Result<CliArgs> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

// ---------------------------------------------------------------------------
// Help / version output (colors for headings only; never converted output)
// ---------------------------------------------------------------------------

/// Render help text. `stdout_tty` selects `Auto` behavior; tests pass an
/// explicit value instead of probing a real terminal.
pub fn render_help(color: ColorMode, stdout_tty: bool) -> String {
    let enabled = color.use_color(stdout_tty);
    let h = |s: &str| paint(s, BOLD, enabled);
    let mut out = String::new();
    out.push_str(&paint(
        "pagina - Markdown/HTML bidirectional converter",
        BOLD,
        enabled,
    ));
    out.push_str("\n\n");
    out.push_str(&h("USAGE:"));
    out.push_str(
        "\n    pagina [--color=auto|always|never] to-html [--gfm] <input.md> <output.html>\n",
    );
    out.push_str("    pagina [--color=auto|always|never] to-md [--gfm] <input.html> <output.md>\n");
    out.push('\n');
    out.push_str("    Use '-' as input to read from stdin, or as output to write to stdout.\n");
    out.push('\n');
    out.push_str(&h("EXAMPLES:"));
    out.push_str("\n    pagina to-html README.md README.html\n");
    out.push_str("    pagina to-html --gfm notes.md notes.html\n");
    out.push_str("    cat file.md | pagina to-html - output.html\n");
    out.push_str("    pagina to-html file.md -\n");
    out.push('\n');
    out.push_str(&h("SUBCOMMANDS:"));
    out.push_str("\n    to-html    Convert Markdown to HTML\n");
    out.push_str("    to-md      Convert HTML to Markdown\n");
    out.push('\n');
    out.push_str(&h("OPTIONS:"));
    out.push_str(
        "\n    --gfm          Enable GFM extensions (task lists, strikethrough, bare autolinks, footnotes, deflists, math).\n",
    );
    out.push_str(
        "                   Pipe tables render with or without --gfm (spec-neutral exception).\n",
    );
    out.push_str(
        "    --color=WHEN   Colorize help and error messages: auto (default, TTY only), always, never.\n",
    );
    out.push_str("                   May also be set with `color` in pagina.toml; --color overrides the file.\n");
    out.push_str("    -h, --help     Print help information\n");
    out.push_str("    -v, --version  Print version information\n");
    out.push('\n');
    out.push_str(&h("CONFIG:"));
    out.push_str(
        "\n    ./pagina.toml, then ~/.config/pagina/config.toml (former wins). Keys: gfm (bool),\n",
    );
    out.push_str(
        "    color (auto|always|never). CLI flags override the file; unknown keys are ignored.\n",
    );
    out.push('\n');
    out.push_str("Note: output preserves raw HTML and javascript:/data: URLs per CommonMark — sanitize before browser/email rendering.\n");
    out
}

pub fn print_help() {
    print_help_with(ColorMode::Auto);
}

/// Print help, colorizing headings only when `color` permits.
pub fn print_help_with(color: ColorMode) {
    print!("{}", render_help(color, std::io::stdout().is_terminal()));
}

/// Render an error line for stderr. Only the `Error:` prefix is painted.
pub fn render_error(err: &Error, color: ColorMode, stderr_tty: bool) -> String {
    let enabled = color.use_color(stderr_tty);
    format!("{}: {}", paint("Error", RED, enabled), err)
}

pub fn print_version() {
    println!("pagina {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unknown_config_keys_are_ignored() {
        let cfg = Config::load_from_str("gfm = true\nweird_key = 123\n[other]\nfoo = 1\n");
        assert!(cfg.gfm);
        assert_eq!(cfg.color, ColorMode::Auto);
    }

    #[test]
    fn bad_config_values_fall_back_to_defaults() {
        let cfg = Config::load_from_str("gfm = maybe\ncolor = rainbow\n");
        assert!(!cfg.gfm);
        assert_eq!(cfg.color, ColorMode::Auto);
    }

    #[test]
    fn config_parses_quoted_color_and_comments() {
        let cfg = Config::load_from_str("# comment\ngfm = true # trailing\ncolor = \"always\"\n");
        assert!(cfg.gfm);
        assert_eq!(cfg.color, ColorMode::Always);
    }

    #[test]
    fn local_config_overrides_home_config() {
        // Merge order: home first, local second (local wins).
        let mut cfg = Config::load_from_str("gfm = false\ncolor = \"never\"\n");
        cfg.apply_str("gfm = true\n");
        assert!(cfg.gfm);
        assert_eq!(cfg.color, ColorMode::Never);
    }

    #[test]
    fn cli_color_overrides_config_color() {
        let config = Config::load_from_str("color = \"never\"\n");
        let args = parse_args_from_with_config(
            &argv(&["pagina", "--color=always", "to-html", "a", "b"]),
            &config,
        )
        .expect("parse failed");
        assert_eq!(args.color, ColorMode::Always);
    }

    #[test]
    fn cli_gfm_overrides_config_gfm() {
        let config = Config::default();
        let args =
            parse_args_from_with_config(&argv(&["pagina", "to-html", "--gfm", "a", "b"]), &config)
                .expect("parse failed");
        match args.subcommand {
            Subcommand::ToHtml { gfm, .. } => assert!(gfm),
            _ => panic!("expected to-html"),
        }
    }

    #[test]
    fn config_gfm_applies_without_flag() {
        let config = Config::load_from_str("gfm = true\n");
        let args = parse_args_from_with_config(&argv(&["pagina", "to-md", "a", "b"]), &config)
            .expect("parse failed");
        match args.subcommand {
            Subcommand::ToMd { gfm, .. } => assert!(gfm),
            _ => panic!("expected to-md"),
        }
    }
}
