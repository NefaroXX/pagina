use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub subcommand: Subcommand,
}

#[derive(Debug, Clone)]
pub enum Subcommand {
    ToHtml { input: String, output: String },
    ToMd { input: String, output: String },
    Help,
    Version,
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
            if args.len() < 4 {
                return Err(Error::MissingArgument(
                    "to-html requires <input.md> <output.html> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(CliArgs {
                subcommand: Subcommand::ToHtml {
                    input: args[2].clone(),
                    output: args[3].clone(),
                },
            })
        }
        "to-md" => {
            if args.len() < 4 {
                return Err(Error::MissingArgument(
                    "to-md requires <input.html> <output.md> (use '-' for stdin/stdout)"
                        .to_string(),
                ));
            }
            Ok(CliArgs {
                subcommand: Subcommand::ToMd {
                    input: args[2].clone(),
                    output: args[3].clone(),
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
    println!("md2html - Markdown/HTML bidirectional converter");
    println!();
    println!("USAGE:");
    println!("    md2html to-html <input.md> <output.html>");
    println!("    md2html to-md <input.html> <output.md>");
    println!();
    println!("    Use '-' as input to read from stdin, or as output to write to stdout.");
    println!();
    println!("EXAMPLES:");
    println!("    md2html to-html README.md README.html");
    println!("    cat file.md | md2html to-html - output.html");
    println!("    md2html to-html file.md -");
    println!();
    println!("SUBCOMMANDS:");
    println!("    to-html    Convert Markdown to HTML");
    println!("    to-md      Convert HTML to Markdown");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help     Print help information");
    println!("    -v, --version  Print version information");
}

pub fn print_version() {
    println!("md2html {}", env!("CARGO_PKG_VERSION"));
}
