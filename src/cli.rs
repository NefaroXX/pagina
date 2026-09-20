use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub subcommand: Subcommand,
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

pub fn parse_args() -> Result<CliArgs> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Ok(CliArgs {
            subcommand: Subcommand::Help,
        });
    }

    match args[1].as_str() {
        "to-html" => {
            let (positionals, gfm) = split_gfm(&args[2..]);
            if positionals.len() < 2 {
                return Err(Error::MissingArgument(
                    "to-html requires <input.md> <output.html> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(CliArgs {
                subcommand: Subcommand::ToHtml {
                    input: positionals[0].clone(),
                    output: positionals[1].clone(),
                    gfm,
                },
            })
        }
        "to-md" => {
            let (positionals, gfm) = split_gfm(&args[2..]);
            if positionals.len() < 2 {
                return Err(Error::MissingArgument(
                    "to-md requires <input.html> <output.md> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(CliArgs {
                subcommand: Subcommand::ToMd {
                    input: positionals[0].clone(),
                    output: positionals[1].clone(),
                    gfm,
                },
            })
        }
        "--help" | "-h" | "help" => Ok(CliArgs {
            subcommand: Subcommand::Help,
        }),
        "--version" | "-v" | "version" => Ok(CliArgs {
            subcommand: Subcommand::Version,
        }),
        other => Err(Error::InvalidSubcommand(other.to_string())),
    }
}

pub fn print_help() {
    println!("pagina - Markdown/HTML bidirectional converter");
    println!();
    println!("USAGE:");
    println!("    pagina to-html [--gfm] <input.md> <output.html>");
    println!("    pagina to-md [--gfm] <input.html> <output.md>");
    println!();
    println!("    Use '-' as input to read from stdin, or as output to write to stdout.");
    println!();
    println!("EXAMPLES:");
    println!("    pagina to-html README.md README.html");
    println!("    pagina to-html --gfm notes.md notes.html");
    println!("    cat file.md | pagina to-html - output.html");
    println!("    pagina to-html file.md -");
    println!();
    println!("SUBCOMMANDS:");
    println!("    to-html    Convert Markdown to HTML");
    println!("    to-md      Convert HTML to Markdown");
    println!();
    println!("OPTIONS:");
    println!(
        "    --gfm          Enable GFM extensions (task lists, strikethrough, bare autolinks)."
    );
    println!(
        "                   Pipe tables render with or without --gfm (spec-neutral exception)."
    );
    println!("    -h, --help     Print help information");
    println!("    -v, --version  Print version information");
    println!();
    println!("Note: output preserves raw HTML and javascript:/data: URLs per CommonMark — sanitize before browser/email rendering.");
}

pub fn print_version() {
    println!("pagina {}", env!("CARGO_PKG_VERSION"));
}
