//! Agent-friendly clap error renderer.
//!
//! By default `clap` prints terse messages like `error: unrecognized subcommand
//! 'creat'` without listing what *is* available, which forces an LLM agent to
//! issue another `--help` round-trip (and sometimes guess wrong again). This
//! module intercepts the most common parse errors and rewrites them with:
//!
//! - the list of valid subcommands / values at the failing position,
//! - a "did you mean" hint when clap already computed one (we surface its
//!   built-in suggestion), and
//! - a copy-pasteable `tip:` line pointing at `--help` for the right scope.
//!
//! Anything we don't have a tailored renderer for falls back to clap's own
//! formatted message so we never lose information.

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Command, CommandFactory, Error as ClapError};
use std::process::ExitCode;

/// Parse the CLI or print a friendly error and exit.
///
/// `parser` is `Cli::try_parse` (or any closure returning a `clap::Result`).
/// On success returns the parsed value; on failure prints to stderr and
/// returns `ExitCode` so `main` can propagate it.
pub fn parse_or_exit<T, F>(parser: F) -> Result<T, ExitCode>
where
    T: CommandFactory,
    F: FnOnce() -> Result<T, ClapError>,
{
    match parser() {
        Ok(cli) => Ok(cli),
        Err(err) => {
            // Help / version requests are not real errors — let clap print as-is.
            match err.kind() {
                ErrorKind::DisplayHelp
                | ErrorKind::DisplayVersion
                | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                    let _ = err.print();
                    return Err(ExitCode::from(0));
                }
                _ => {}
            }

            let mut cmd = T::command();
            let rendered = render_friendly(&mut cmd, &err);
            eprintln!("{}", rendered.trim_end());
            Err(ExitCode::from(2))
        }
    }
}

/// Render an agent-friendly message for `err`. Falls back to clap's own
/// rendering if we don't have a tailored handler.
fn render_friendly(root: &mut Command, err: &ClapError) -> String {
    let usage_path = extract_usage_path(err);
    let scope = usage_path.join(" ");
    let scoped = resolve_subcommand(root, &usage_path);

    match err.kind() {
        ErrorKind::InvalidSubcommand => render_invalid_subcommand(err, &scope, scoped),
        ErrorKind::UnknownArgument => {
            // Real unknown flags (start with '-') keep clap's default rendering;
            // bare-word "unknown args" we treat as bad subcommand.
            let bad = invalid_subcommand(err);
            if bad.as_deref().map(|s| s.starts_with('-')).unwrap_or(true) {
                err.render().to_string()
            } else {
                render_invalid_subcommand(err, &scope, scoped)
            }
        }
        ErrorKind::InvalidValue => render_invalid_value(err, &scope),
        ErrorKind::MissingRequiredArgument => render_missing_required(err, &scope),
        _ => err.render().to_string(),
    }
}

/// Pull `["vidu-cli", "task", ...]`-style breadcrumb out of clap's error
/// context, defaulting to `["vidu-cli"]` if clap didn't populate Usage.
///
/// Some error kinds (notably `InvalidValue`) leave the `Usage` context
/// empty; in that case we fall back to scraping the `Usage:` line out of
/// `err.render()`. Returns just `["vidu-cli"]` when no Usage is available
/// anywhere — callers should treat that as "scope unknown".
fn extract_usage_path(err: &ClapError) -> Vec<String> {
    // Usage ships as `ContextValue::StyledStr`, so use the display-based
    // helper rather than `ctx_string` which now only accepts `String`.
    if let Some(usage) = ctx_displayed(err, ContextKind::Usage) {
        let p = parse_usage_line(&strip_ansi(&usage));
        if p.len() > 1 {
            return p;
        }
    }
    let rendered = strip_ansi(&err.render().to_string());
    if let Some(line) = rendered
        .lines()
        .find(|l| l.trim_start().starts_with("Usage:"))
    {
        let p = parse_usage_line(line);
        if p.len() > 1 {
            return p;
        }
    }
    vec!["vidu-cli".to_string()]
}

/// Strip ANSI escape sequences so `Usage:` matching works regardless of color.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            // Other escape: skip one char.
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Clap's `Usage` context is a multi-line string starting with
/// `Usage: vidu-cli <a> <b> [OPTIONS]`. We only need the leading binary +
/// subcommand chain to look up siblings.
fn parse_usage_line(usage: &str) -> Vec<String> {
    let line = usage
        .lines()
        .find(|l| l.trim_start().starts_with("Usage:"))
        .unwrap_or(usage);
    let mut out = Vec::new();
    for tok in line.split_whitespace().skip(1) {
        if tok.starts_with('<') || tok.starts_with('[') || tok.starts_with('-') {
            break;
        }
        if tok == "Usage:" {
            continue;
        }
        out.push(tok.to_string());
    }
    if out.is_empty() {
        out.push("vidu-cli".to_string());
    }
    out
}

/// Walk `cmd` down `path` (skipping the binary name at index 0) and return
/// the deepest `Command` we could resolve. Falls back to `cmd` itself.
fn resolve_subcommand<'a>(cmd: &'a mut Command, path: &[String]) -> &'a Command {
    cmd.build();
    let mut cur: &Command = cmd;
    for name in path.iter().skip(1) {
        match cur.find_subcommand(name) {
            Some(next) => cur = next,
            None => break,
        }
    }
    cur
}

// --- ContextValue extraction helpers -------------------------------------
// `ClapError::get` returns `Option<&ContextValue>`, so all the helpers below
// clone the inner data. Cheap because errors run once at the very end.
//
// We deliberately keep one helper per ContextValue variant we care about so
// callers can decide what's a hit and what's a fallback. Mixing variants
// inside a single helper (e.g. coercing `Strings(vec)` into `"a, b"` via
// `to_string()`) leaks rendering details into otherwise-clean fields like
// `SuggestedValue` and produces noise such as `tip: did you mean '"a", "b"'?`.

/// Read `ContextKind` if and only if it's `ContextValue::String`. Returns
/// `None` for any other variant so callers can fall back deliberately.
fn ctx_string(err: &ClapError, kind: ContextKind) -> Option<String> {
    match err.get(kind)? {
        ContextValue::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Read `ContextKind` if it's `ContextValue::Strings`. Returns an empty
/// vec for any other variant.
fn ctx_strings(err: &ClapError, kind: ContextKind) -> Vec<String> {
    match err.get(kind) {
        Some(ContextValue::Strings(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Read `ContextKind` regardless of variant by formatting it via `Display`.
/// Used for fields like `Usage` which clap ships as `StyledStr`. Returns
/// `None` if the rendered text is empty.
fn ctx_displayed(err: &ClapError, kind: ContextKind) -> Option<String> {
    let v = err.get(kind)?;
    let s = v.to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn invalid_subcommand(err: &ClapError) -> Option<String> {
    ctx_string(err, ContextKind::InvalidSubcommand)
        .or_else(|| ctx_string(err, ContextKind::InvalidArg))
}

fn invalid_arg(err: &ClapError) -> Option<String> {
    ctx_string(err, ContextKind::InvalidArg)
}

fn invalid_value(err: &ClapError) -> Option<String> {
    ctx_string(err, ContextKind::InvalidValue)
}

/// Try to surface clap's built-in "did you mean X?" hint, regardless of
/// whether it was filed under SuggestedSubcommand or SuggestedValue and
/// whether it was a single string or a list.
fn suggested_value(err: &ClapError) -> Option<String> {
    for kind in [ContextKind::SuggestedSubcommand, ContextKind::SuggestedValue] {
        if let Some(s) = ctx_string(err, kind) {
            return Some(s);
        }
        let items = ctx_strings(err, kind);
        if let Some(first) = items.into_iter().next() {
            return Some(first);
        }
    }
    None
}

fn missing_required(err: &ClapError) -> Vec<String> {
    ctx_strings(err, ContextKind::InvalidArg)
        .into_iter()
        .map(|s| strip_ansi(&s))
        .collect()
}

// --- Renderers -----------------------------------------------------------

fn render_invalid_subcommand(err: &ClapError, scope: &str, scoped: &Command) -> String {
    let bad = invalid_subcommand(err).unwrap_or_else(|| "<unknown>".to_string());

    let mut available: Vec<String> = scoped
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .map(|c| c.get_name().to_string())
        .collect();
    available.sort();

    if available.is_empty() {
        // No siblings to list — defer to clap's default message.
        return err.render().to_string();
    }

    let mut out = String::new();
    out.push_str(&format!(
        "error: unrecognized subcommand '{bad}' under '{scope}'\n"
    ));
    out.push_str(&format!("  available: {}\n", available.join(", ")));
    if let Some(hint) = suggested_value(err) {
        out.push_str(&format!(
            "  tip: did you mean '{hint}'?  (run: {scope} {hint} --help)\n"
        ));
    } else {
        out.push_str(&format!("  tip: run `{scope} --help` to see usage\n"));
    }
    out
}

fn render_invalid_value(err: &ClapError, scope: &str) -> String {
    let arg = invalid_arg(err).unwrap_or_default();
    let bad = invalid_value(err).unwrap_or_default();
    let valid = ctx_strings(err, ContextKind::ValidValue);

    let mut out = String::new();
    if arg.is_empty() {
        return err.render().to_string();
    }
    if bad.is_empty() {
        out.push_str(&format!("error: invalid value for '{arg}'\n"));
    } else {
        out.push_str(&format!("error: invalid value '{bad}' for '{arg}'\n"));
    }
    if !valid.is_empty() {
        out.push_str(&format!("  possible values: {}\n", valid.join(", ")));
    }
    if let Some(hint) = suggested_value(err) {
        out.push_str(&format!("  tip: did you mean '{hint}'?\n"));
    }
    // Only emit a `<scope> --help` tip if we actually identified the
    // subcommand path. For InvalidValue, clap doesn't always ship a Usage
    // breadcrumb, in which case `scope == "vidu-cli"` and the help target
    // would be misleading; skip it instead of guessing wrong.
    if scope != "vidu-cli" {
        out.push_str(&format!("  tip: run `{scope} --help` for parameter docs\n"));
    }
    out
}

fn render_missing_required(err: &ClapError, scope: &str) -> String {
    let missing = missing_required(err);
    if missing.is_empty() {
        return err.render().to_string();
    }
    let mut out = String::new();
    out.push_str("error: missing required arguments:\n");
    for arg in &missing {
        out.push_str(&format!("  - {arg}\n"));
    }
    out.push_str(&format!("  tip: run `{scope} --help` for parameter docs\n"));
    out
}

#[cfg(test)]
mod tests {
    //! Tests use a stand-in CLI definition that exercises all the renderer
    //! branches. We deliberately avoid wiring up the full `Cli` struct so the
    //! tests stay close to the renderer logic.
    use super::*;
    use clap::builder::PossibleValuesParser;
    use clap::{Parser, Subcommand};

    #[derive(Parser, Debug)]
    #[command(name = "vidu-cli")]
    struct TestCli {
        #[command(subcommand)]
        g: Option<G>,
    }

    #[derive(Subcommand, Debug)]
    enum G {
        Task {
            #[command(subcommand)]
            a: T,
        },
        Quota {
            #[command(subcommand)]
            a: Q,
        },
    }

    #[derive(Subcommand, Debug)]
    enum T {
        Submit {
            #[arg(long = "type",
                value_parser = PossibleValuesParser::new(
                    ["text2image", "img2video", "character2video"]))]
            task_type: String,
            #[arg(long)]
            model_version: String,
        },
        Get,
    }

    #[derive(Subcommand, Debug)]
    enum Q {
        Pass,
        Credit,
    }

    fn render_for(args: &[&str]) -> String {
        match TestCli::try_parse_from(args) {
            Ok(_) => panic!("expected parse error for {:?}", args),
            Err(e) => {
                let mut cmd = TestCli::command();
                render_friendly(&mut cmd, &e)
            }
        }
    }

    #[test]
    fn invalid_top_level_subcommand_lists_available_and_suggests() {
        let out = render_for(&["vidu-cli", "tasks"]);
        assert!(out.contains("unrecognized subcommand 'tasks'"), "got: {out}");
        assert!(out.contains("under 'vidu-cli'"), "got: {out}");
        assert!(out.contains("available: quota, task"), "got: {out}");
        assert!(out.contains("did you mean 'task'"), "got: {out}");
    }

    #[test]
    fn invalid_nested_subcommand_uses_correct_scope() {
        let out = render_for(&["vidu-cli", "task", "creat"]);
        assert!(out.contains("under 'vidu-cli task'"), "got: {out}");
        assert!(out.contains("available: get, submit"), "got: {out}");
    }

    #[test]
    fn invalid_value_lists_possible_values_and_suggestion() {
        let out = render_for(&[
            "vidu-cli",
            "task",
            "submit",
            "--type",
            "img2img",
            "--model-version",
            "3.2",
        ]);
        assert!(out.contains("invalid value 'img2img'"), "got: {out}");
        assert!(
            out.contains("possible values: text2image, img2video, character2video"),
            "got: {out}"
        );
        assert!(out.contains("did you mean 'img2video'"), "got: {out}");
    }

    #[test]
    fn missing_required_lists_each_arg_with_help_tip() {
        let out = render_for(&["vidu-cli", "task", "submit"]);
        assert!(out.contains("missing required arguments:"), "got: {out}");
        assert!(out.contains("--type <TASK_TYPE>"), "got: {out}");
        assert!(
            out.contains("`vidu-cli task submit --help`"),
            "got: {out}"
        );
    }

    #[test]
    fn unknown_flag_falls_back_to_clap_default() {
        // `--typo` is a real unknown flag; we keep clap's own message because
        // it already includes a "did you mean" suggestion.
        let out = render_for(&["vidu-cli", "task", "submit", "--typo", "x"]);
        assert!(out.contains("--typo"), "got: {out}");
        // Negative assertions: ensure the renderer for InvalidSubcommand did
        // NOT take over (those messages contain "under '" and "available:").
        assert!(
            !out.contains("under 'vidu-cli"),
            "should not be rewritten as bad subcommand: {out}"
        );
        assert!(
            !out.contains("available:"),
            "should not be rewritten as bad subcommand: {out}"
        );
    }

    #[test]
    fn parse_usage_line_extracts_path() {
        let p = parse_usage_line("Usage: vidu-cli task submit --type <TYPE>");
        assert_eq!(p, vec!["vidu-cli", "task", "submit"]);
    }

    #[test]
    fn parse_usage_line_stops_at_placeholders() {
        let p = parse_usage_line("Usage: vidu-cli quota <COMMAND>");
        assert_eq!(p, vec!["vidu-cli", "quota"]);
    }

    #[test]
    fn strip_ansi_removes_csi_sequences() {
        let s = "\u{1b}[1m\u{1b}[4mUsage:\u{1b}[0m \u{1b}[1mvidu-cli\u{1b}[0m [COMMAND]";
        assert_eq!(strip_ansi(s), "Usage: vidu-cli [COMMAND]");
    }
}
