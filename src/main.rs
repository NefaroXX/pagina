use md2html::cli::{parse_args, print_help, print_version, Subcommand};
use md2html::error::{Error, Result};
use std::fs;
use std::io;

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
        Subcommand::ToHtml { input, output } => convert_to_html(&input, &output),
        Subcommand::ToMd { input, output } => convert_to_md(&input, &output),
    }
}

fn convert_to_html(input_path: &str, output_path: &str) -> Result<()> {
    let input = fs::read_to_string(input_path).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("Failed to read '{}': {}", input_path, e),
        ))
    })?;

    let html = md2html::markdown_to_html::convert(&input)?;

    fs::write(output_path, html).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("Failed to write '{}': {}", output_path, e),
        ))
    })?;

    println!("Converted {} -> {}", input_path, output_path);
    Ok(())
}

fn convert_to_md(input_path: &str, output_path: &str) -> Result<()> {
    let input = fs::read_to_string(input_path).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("Failed to read '{}': {}", input_path, e),
        ))
    })?;

    let markdown = md2html::html_to_markdown::convert(&input)?;

    fs::write(output_path, markdown).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("Failed to write '{}': {}", output_path, e),
        ))
    })?;

    println!("Converted {} -> {}", input_path, output_path);
    Ok(())
}
