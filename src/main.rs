use pagina::cli::{parse_args, print_help, print_version, Subcommand};
use pagina::error::{Error, Result};
use std::fs;
use std::io::{self, Read, Write};

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;

    match args.subcommand {
        Subcommand::Help => {
            print_help();
            Ok(())
        }
        Subcommand::Version => {
            print_version();
            Ok(())
        }
        Subcommand::ToHtml { input, output, gfm } => convert_to_html(&input, &output, gfm),
        Subcommand::ToMd { input, output, gfm } => convert_to_md(&input, &output, gfm),
    }
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("Failed to read stdin: {}", e),
            ))
        })?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("Failed to read '{}': {}", path, e),
            ))
        })
    }
}

fn write_output(path: &str, content: &str) -> Result<()> {
    if path == "-" {
        io::stdout().write_all(content.as_bytes()).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("Failed to write stdout: {}", e),
            ))
        })?;
    } else {
        fs::write(path, content).map_err(|e| {
            Error::Io(io::Error::new(
                e.kind(),
                format!("Failed to write '{}': {}", path, e),
            ))
        })?;
    }
    Ok(())
}

fn convert_to_html(input_path: &str, output_path: &str, gfm: bool) -> Result<()> {
    let input = read_input(input_path)?;
    let html = if gfm {
        pagina::markdown_to_html::convert_gfm(&input)?
    } else {
        pagina::markdown_to_html::convert(&input)?
    };
    write_output(output_path, &html)?;
    if input_path != "-" && output_path != "-" {
        println!("Converted {} -> {}", input_path, output_path);
    }
    Ok(())
}

fn convert_to_md(input_path: &str, output_path: &str, gfm: bool) -> Result<()> {
    let input = read_input(input_path)?;
    let markdown = if gfm {
        pagina::html_to_markdown::convert_gfm(&input)?
    } else {
        pagina::html_to_markdown::convert(&input)?
    };
    write_output(output_path, &markdown)?;
    if input_path != "-" && output_path != "-" {
        println!("Converted {} -> {}", input_path, output_path);
    }
    Ok(())
}
