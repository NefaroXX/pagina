//! CLI integration tests: arg parsing, `--color`, `pagina.toml` config,
//! completion scripts, and end-to-end binary behavior.
//!
//! Pure parsing tests use [`pagina::cli::parse_args_from_with_config`] with
//! an explicit [`pagina::cli::Config`] so they never touch the filesystem.
//! The two precedence tests that must observe real files serialize on a
//! process-wide mutex because they mutate CWD and `HOME`/`USERPROFILE`.

use pagina::cli::{
    parse_args_from, parse_args_from_with_config, render_error, render_help, ColorMode, Config,
    Subcommand,
};
use pagina::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn default_cfg() -> Config {
    Config::default()
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pagina"))
}

fn manifest_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

// ---------------------------------------------------------------------------
// parse_args: subcommands and --gfm
// ---------------------------------------------------------------------------

#[test]
fn no_args_yields_help() {
    let args = parse_args_from_with_config(&argv(&["pagina"]), &default_cfg()).unwrap();
    assert!(matches!(args.subcommand, Subcommand::Help));
    assert_eq!(args.color, ColorMode::Auto);
}

#[test]
fn to_html_positionals() {
    let args = parse_args_from_with_config(
        &argv(&["pagina", "to-html", "in.md", "out.html"]),
        &default_cfg(),
    )
    .unwrap();
    match args.subcommand {
        Subcommand::ToHtml { input, output, gfm } => {
            assert_eq!(input, "in.md");
            assert_eq!(output, "out.html");
            assert!(!gfm);
        }
        _ => panic!("expected to-html"),
    }
}

#[test]
fn to_md_positionals() {
    let args = parse_args_from_with_config(
        &argv(&["pagina", "to-md", "in.html", "out.md"]),
        &default_cfg(),
    )
    .unwrap();
    match args.subcommand {
        Subcommand::ToMd { input, output, gfm } => {
            assert_eq!(input, "in.html");
            assert_eq!(output, "out.md");
            assert!(!gfm);
        }
        _ => panic!("expected to-md"),
    }
}

#[test]
fn gfm_accepted_before_and_after_positionals() {
    for extra in [
        vec!["pagina", "to-html", "--gfm", "a", "b"],
        vec!["pagina", "to-html", "a", "b", "--gfm"],
        vec!["pagina", "to-html", "a", "--gfm", "b"],
    ] {
        let args = parse_args_from_with_config(&argv(&extra), &default_cfg()).unwrap();
        match args.subcommand {
            Subcommand::ToHtml { gfm, .. } => assert!(gfm, "gfm missing for {:?}", extra),
            _ => panic!("expected to-html"),
        }
    }
}

#[test]
fn help_and_version_aliases() {
    for flag in ["--help", "-h", "help"] {
        let args = parse_args_from_with_config(&argv(&["pagina", flag]), &default_cfg()).unwrap();
        assert!(matches!(args.subcommand, Subcommand::Help), "flag {}", flag);
    }
    for flag in ["--version", "-v", "version"] {
        let args = parse_args_from_with_config(&argv(&["pagina", flag]), &default_cfg()).unwrap();
        assert!(
            matches!(args.subcommand, Subcommand::Version),
            "flag {}",
            flag
        );
    }
}

#[test]
fn invalid_subcommand_errors() {
    let err = parse_args_from_with_config(&argv(&["pagina", "frobnicate"]), &default_cfg())
        .expect_err("expected error");
    assert!(matches!(err, Error::InvalidSubcommand(_)));
}

#[test]
fn missing_positionals_error() {
    let err =
        parse_args_from_with_config(&argv(&["pagina", "to-html", "only-one"]), &default_cfg())
            .expect_err("expected error");
    assert!(matches!(err, Error::MissingArgument(_)));
}

// ---------------------------------------------------------------------------
// --color flag
// ---------------------------------------------------------------------------

#[test]
fn color_eq_forms_all_modes() {
    for (raw, expected) in [
        ("--color=auto", ColorMode::Auto),
        ("--color=always", ColorMode::Always),
        ("--color=never", ColorMode::Never),
    ] {
        // Global position (before subcommand).
        let args = parse_args_from_with_config(
            &argv(&["pagina", raw, "to-html", "a", "b"]),
            &default_cfg(),
        )
        .unwrap();
        assert_eq!(args.color, expected, "global {}", raw);
        // Per-subcommand position (after subcommand).
        let args = parse_args_from_with_config(
            &argv(&["pagina", "to-html", raw, "a", "b"]),
            &default_cfg(),
        )
        .unwrap();
        assert_eq!(args.color, expected, "local {}", raw);
    }
}

#[test]
fn color_space_form() {
    let args = parse_args_from_with_config(
        &argv(&["pagina", "--color", "never", "to-html", "a", "b"]),
        &default_cfg(),
    )
    .unwrap();
    assert_eq!(args.color, ColorMode::Never);

    let args = parse_args_from_with_config(
        &argv(&["pagina", "to-md", "--color", "always", "a", "b"]),
        &default_cfg(),
    )
    .unwrap();
    assert_eq!(args.color, ColorMode::Always);
}

#[test]
fn color_case_insensitive() {
    let args = parse_args_from_with_config(
        &argv(&["pagina", "--color=ALWAYS", "to-html", "a", "b"]),
        &default_cfg(),
    )
    .unwrap();
    assert_eq!(args.color, ColorMode::Always);
}

#[test]
fn invalid_color_value_errors() {
    for extra in [
        vec!["pagina", "--color=rainbow", "to-html", "a", "b"],
        vec!["pagina", "to-html", "--color", "rainbow", "a", "b"],
    ] {
        let err =
            parse_args_from_with_config(&argv(&extra), &default_cfg()).expect_err("expected error");
        assert!(matches!(err, Error::InvalidInput(_)), "args {:?}", extra);
    }
}

#[test]
fn missing_color_value_errors() {
    let err = parse_args_from_with_config(&argv(&["pagina", "to-html", "--color"]), &default_cfg())
        .expect_err("expected error");
    assert!(matches!(err, Error::MissingArgument(_)));
}

// ---------------------------------------------------------------------------
// Config: pure precedence and tolerance
// ---------------------------------------------------------------------------

#[test]
fn config_gfm_flows_into_parse() {
    let config = Config::load_from_str("gfm = true\n");
    let args =
        parse_args_from_with_config(&argv(&["pagina", "to-html", "a", "b"]), &config).unwrap();
    match args.subcommand {
        Subcommand::ToHtml { gfm, .. } => assert!(gfm),
        _ => panic!("expected to-html"),
    }
}

#[test]
fn cli_gfm_flag_wins_over_config_false() {
    let config = Config::load_from_str("gfm = false\n");
    let args = parse_args_from_with_config(&argv(&["pagina", "to-md", "--gfm", "a", "b"]), &config)
        .unwrap();
    match args.subcommand {
        Subcommand::ToMd { gfm, .. } => assert!(gfm),
        _ => panic!("expected to-md"),
    }
}

#[test]
fn cli_color_wins_over_config_color() {
    let config = Config::load_from_str("color = \"never\"\n");
    let args = parse_args_from_with_config(
        &argv(&["pagina", "--color=always", "to-html", "a", "b"]),
        &config,
    )
    .unwrap();
    assert_eq!(args.color, ColorMode::Always);
}

#[test]
fn config_color_used_when_flag_absent() {
    let config = Config::load_from_str("color = always\n");
    let args =
        parse_args_from_with_config(&argv(&["pagina", "to-html", "a", "b"]), &config).unwrap();
    assert_eq!(args.color, ColorMode::Always);
}

#[test]
fn config_unknown_keys_and_bad_values_ignored() {
    let cfg = Config::load_from_str(
        "gfm = true\nbogus_key = 123\ncolor = \"rainbow\"\n[table]\nfoo = 1\nno-equals-line\n",
    );
    assert!(cfg.gfm);
    // Bad color falls back to default instead of erroring.
    assert_eq!(cfg.color, ColorMode::Auto);
}

// ---------------------------------------------------------------------------
// Config: filesystem precedence via temp dirs + std::env
// ---------------------------------------------------------------------------

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "pagina-cli-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

struct EnvGuard {
    cwd: PathBuf,
    home: Option<String>,
    userprofile: Option<String>,
}

impl EnvGuard {
    fn take() -> Self {
        EnvGuard {
            cwd: std::env::current_dir().expect("current_dir failed"),
            home: std::env::var("HOME").ok(),
            userprofile: std::env::var("USERPROFILE").ok(),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.cwd);
        match &self.home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match &self.userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}

fn set_home(dir: &std::path::Path) {
    let s = dir.to_string_lossy().into_owned();
    std::env::set_var("HOME", &s);
    std::env::set_var("USERPROFILE", &s);
}

#[test]
fn config_file_precedence_local_over_home() {
    let _guard = env_lock().lock().unwrap();
    let _env = EnvGuard::take();

    let home = unique_dir("home");
    let work = unique_dir("work");
    std::fs::create_dir_all(home.join(".config").join("pagina")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        home.join(".config").join("pagina").join("config.toml"),
        "gfm = false\ncolor = \"never\"\n",
    )
    .unwrap();
    std::fs::write(work.join("pagina.toml"), "gfm = true\nunknown = 1\n").unwrap();

    set_home(&home);
    std::env::set_current_dir(&work).unwrap();

    // Local wins for gfm; home still supplies color (merge).
    let cfg = Config::load();
    assert!(cfg.gfm, "local pagina.toml should win");
    assert_eq!(cfg.color, ColorMode::Never);

    // CLI flags still override both files.
    let parsed = parse_args_from(&argv(&["pagina", "--color=always", "to-html", "a", "b"]))
        .expect("parse failed");
    assert_eq!(parsed.color, ColorMode::Always);
    match parsed.subcommand {
        Subcommand::ToHtml { gfm, .. } => assert!(gfm),
        _ => panic!("expected to-html"),
    }

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn config_home_used_when_no_local_file() {
    let _guard = env_lock().lock().unwrap();
    let _env = EnvGuard::take();

    let home = unique_dir("home2");
    let work = unique_dir("work2");
    std::fs::create_dir_all(home.join(".config").join("pagina")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        home.join(".config").join("pagina").join("config.toml"),
        "gfm = true\n",
    )
    .unwrap();

    set_home(&home);
    std::env::set_current_dir(&work).unwrap();

    let cfg = Config::load();
    assert!(cfg.gfm, "home config should apply without local file");

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&work);
}

// ---------------------------------------------------------------------------
// Help / error rendering: colors only when enabled
// ---------------------------------------------------------------------------

#[test]
fn help_never_has_no_escapes() {
    let text = render_help(ColorMode::Never, true);
    assert!(!text.contains('\x1b'), "never must be plain");
    assert!(text.contains("USAGE:"));
    assert!(text.contains("--color="));
    assert!(text.contains("pagina.toml"));
}

#[test]
fn help_always_has_escapes_even_when_piped() {
    let text = render_help(ColorMode::Always, false);
    assert!(text.contains('\x1b'), "always must colorize");
}

#[test]
fn help_auto_follows_tty() {
    assert!(!render_help(ColorMode::Auto, false).contains('\x1b'));
    assert!(render_help(ColorMode::Auto, true).contains('\x1b'));
}

#[test]
fn error_rendering_respects_color() {
    let err = Error::InvalidSubcommand("nope".to_string());
    assert!(!render_error(&err, ColorMode::Never, true).contains('\x1b'));
    assert!(render_error(&err, ColorMode::Always, false).contains('\x1b'));
    // Message body survives in both modes.
    assert!(render_error(&err, ColorMode::Never, false).contains("nope"));
}

// ---------------------------------------------------------------------------
// Static assets: completions + man page
// ---------------------------------------------------------------------------

#[test]
fn completion_files_exist_and_cover_cli() {
    for rel in [
        "completions/pagina.bash",
        "completions/pagina.zsh",
        "completions/pagina.fish",
        "completions/pagina.ps1",
    ] {
        let path = manifest_file(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing completion file {}", rel));
        for token in [
            "to-html",
            "to-md",
            "--gfm",
            "--color",
            "--help",
            "--version",
        ] {
            assert!(
                text.contains(token),
                "{} must mention {} (exact cli.rs flag name)",
                rel,
                token
            );
        }
    }
}

#[test]
fn man_page_has_required_sections() {
    let text =
        std::fs::read_to_string(manifest_file("doc/pagina.1")).expect("missing doc/pagina.1");
    for section in [
        "NAME",
        "SYNOPSIS",
        "DESCRIPTION",
        "OPTIONS",
        "EXAMPLES",
        "SEE ALSO",
    ] {
        assert!(text.contains(section), "man page must contain {}", section);
    }
    for token in ["to-html", "to-md", "--gfm", "--color"] {
        assert!(text.contains(token), "man page must mention {}", token);
    }
}

// ---------------------------------------------------------------------------
// Binary end-to-end (zero-dep: std::process only)
// ---------------------------------------------------------------------------

#[test]
fn binary_help_lists_color_flag() {
    let out = Command::new(bin())
        .arg("--help")
        .output()
        .expect("failed to run pagina --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("USAGE:"));
    assert!(stdout.contains("--color="));
}

#[test]
fn binary_converts_with_color_flag() {
    let dir = unique_dir("bin");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("in.md"), "# Hi\n").unwrap();
    let status = Command::new(bin())
        .arg("--color=never")
        .arg("to-html")
        .arg(dir.join("in.md"))
        .arg(dir.join("out.html"))
        .status()
        .expect("failed to run pagina");
    assert!(status.success());
    let html = std::fs::read_to_string(dir.join("out.html")).unwrap();
    assert!(html.contains("<h1>Hi</h1>"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn binary_rejects_bad_color() {
    let out = Command::new(bin())
        .arg("--color=rainbow")
        .arg("to-html")
        .arg("a")
        .arg("b")
        .output()
        .expect("failed to run pagina");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--color"));
}
