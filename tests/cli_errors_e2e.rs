//! End-to-end tests for the agent-friendly CLI parse error renderer.
//!
//! These spawn the real `vidu-cli` binary and assert on its stderr / exit
//! code, locking in the contract that LLM agents (and the docs in
//! `vidu-skills`) rely on. Unit tests in `cli_errors::tests` cover the
//! renderer in isolation; this file covers the wiring through `main()` +
//! `parse_or_exit` + `ExitCode` propagation.
//!
//! No network or auth required — all of these fail at parse time before
//! any HTTP call.

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

fn cli() -> Command {
    Command::cargo_bin("vidu-cli").expect("vidu-cli binary should build")
}

// --- Success-path exit codes ---------------------------------------------

#[test]
fn help_flag_exits_zero() {
    cli().arg("--help").assert().success();
}

#[test]
fn version_flag_exits_zero() {
    cli().arg("--version").assert().success();
}

#[test]
fn no_args_prints_help_and_exits_zero() {
    // `main()` falls through to `Cli::command().print_help()` when no
    // subcommand is given. Should NOT exit 2.
    cli()
        .assert()
        .success()
        .stdout(contains("Usage:").or(contains("Vidu")));
}

// --- InvalidSubcommand at top level --------------------------------------

#[test]
fn typo_top_level_subcommand_lists_available_and_suggests() {
    cli()
        .arg("tasks")
        .assert()
        .code(2)
        .stderr(contains("unrecognized subcommand 'tasks'"))
        .stderr(contains("under 'vidu-cli'"))
        .stderr(contains("available:"))
        .stderr(contains("did you mean 'task'"));
}

// --- InvalidSubcommand at nested level -----------------------------------

#[test]
fn typo_nested_subcommand_uses_correct_scope() {
    cli()
        .args(["task", "creat"])
        .assert()
        .code(2)
        .stderr(contains("under 'vidu-cli task'"))
        .stderr(contains("available:"))
        .stderr(contains("submit"));
}

#[test]
fn typo_quota_subcommand_lists_pass_and_credit() {
    cli()
        .args(["quota", "balance"])
        .assert()
        .code(2)
        .stderr(contains("under 'vidu-cli quota'"))
        .stderr(contains("available: credit, pass"));
}

// --- InvalidValue (PossibleValuesParser) ---------------------------------

#[test]
fn invalid_type_value_lists_possible_values_and_suggestion() {
    cli()
        .args([
            "task", "submit",
            "--type", "img2img",
            "--prompt", "x",
            "--model-version", "3.2",
            "--resolution", "1080p",
        ])
        .assert()
        .code(2)
        .stderr(contains("invalid value 'img2img'"))
        .stderr(contains("Valid:"))
        .stderr(contains("img2video"));
}

#[test]
fn invalid_resolution_value_caught_at_parse_time() {
    cli()
        .args([
            "task", "submit",
            "--type", "text2video",
            "--prompt", "x",
            "--model-version", "3.2",
            "--resolution", "8k",
        ])
        .assert()
        .code(2)
        .stderr(contains("invalid value '8k'"))
        .stderr(contains("possible values:"))
        .stderr(contains("1080p"));
}

#[test]
fn invalid_language_boost_caught_at_parse_time() {
    // Regression: --language-boost used to skip clap parse and only fail
    // inside validate_tts_language_boost (after the API client kicked in).
    // It must now be caught by PossibleValuesParser at parse time.
    cli()
        .args([
            "task", "tts",
            "--prompt", "hello",
            "--voice-id", "x",
            "--language-boost", "Klingon",
        ])
        .assert()
        .code(2)
        .stderr(contains("invalid value 'Klingon'"))
        .stderr(contains("possible values:"))
        .stderr(contains("English"));
}

// --- MissingRequiredArgument --------------------------------------------

#[test]
fn missing_required_lists_each_arg_with_help_tip() {
    cli()
        .args(["task", "submit"])
        .assert()
        .code(2)
        .stderr(contains("missing required arguments:"))
        .stderr(contains("--type"))
        .stderr(contains("--model-version"))
        .stderr(contains("--resolution"))
        .stderr(contains("`vidu-cli task submit --help`"));
}
