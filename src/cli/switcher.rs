//! Native backend switching built into the forked CLI.
//!
//! Innov already owns the PTYs and native agent sessions. This command adds a
//! logical handoff layer on top of that runtime without invoking another
//! `innov` executable.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::api::schema::{
    AgentPromptParams, AgentPromptWaitOptions, AgentReadParams, AgentStartParams, AgentStatus,
    AgentTarget, Method, PaneSplitParams, PaneTarget, ReadFormat, ReadSource, Request,
    SplitDirection,
};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_CONTEXT_TOKENS: usize = 3_500;
const CONTEXT_CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, PartialEq, Eq)]
struct SwitchOptions {
    source: String,
    backend: String,
    no_handoff: bool,
    timeout_ms: u64,
    context_tokens: usize,
}

pub(super) fn run_switch_command(args: &[String]) -> io::Result<i32> {
    let options = match parse_switch_options(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print_switch_help();
            return Ok(0);
        }
        Err(error) => {
            eprintln!("{error}");
            return Ok(2);
        }
    };

    switch_agent(
        &options.source,
        &options.backend,
        options.no_handoff,
        options.timeout_ms,
        options.context_tokens,
    )
}

fn parse_switch_options(args: &[String]) -> Result<Option<SwitchOptions>, String> {
    let Some(source) = args.first().filter(|value| !value.starts_with('-')) else {
        return Err(
            "usage: innov switch <agent-target> --to <codex|claude|agy|profile-alias> [OPTIONS]"
                .into(),
        );
    };

    let mut backend = None;
    let mut no_handoff = false;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut context_tokens = DEFAULT_CONTEXT_TOKENS;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "help" | "--help" | "-h" => return Ok(None),
            "--to" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --to".into());
                };
                backend = Some(value.clone());
                index += 2;
            }
            "--no-handoff" => {
                no_handoff = true;
                index += 1;
            }
            "--timeout" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --timeout".into());
                };
                timeout_ms =
                    super::parse_u64_flag("--timeout", value).map_err(|error| error.to_string())?;
                if !(3_001..=300_000).contains(&timeout_ms) {
                    return Err("--timeout must be greater than 3000 and at most 300000".into());
                }
                index += 2;
            }
            "--context-tokens" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("missing value for --context-tokens".into());
                };
                context_tokens = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid value for --context-tokens: {value}"))?;
                if context_tokens == 0 {
                    return Err("--context-tokens must be greater than zero".into());
                }
                index += 2;
            }
            option => return Err(format!("unknown option: {option}")),
        }
    }

    let Some(backend) = backend else {
        return Err("missing required --to BACKEND".into());
    };
    if backend
        .chars()
        .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err("--to must be a backend name or shell-safe profile alias".into());
    }

    Ok(Some(SwitchOptions {
        source: source.clone(),
        backend,
        no_handoff,
        timeout_ms,
        context_tokens,
    }))
}

fn switch_agent(
    source: &str,
    backend: &str,
    no_handoff: bool,
    timeout_ms: u64,
    context_tokens: usize,
) -> io::Result<i32> {
    let source_response = request(Method::AgentGet(AgentTarget {
        target: source.to_owned(),
    }))?;
    let source_agent = result_field(&source_response, "agent")?;
    let source_pane = string_field(source_agent, "pane_id")?;
    let workspace_id = string_field(source_agent, "workspace_id")?;
    let cwd = source_agent
        .get("foreground_cwd")
        .and_then(Value::as_str)
        .or_else(|| source_agent.get("cwd").and_then(Value::as_str))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let target = super::profile::resolve_switch_target(backend, &home)?;
    let source_kind = source_agent
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let recent_output = read_recent(source)?;

    let handoff_path = if no_handoff {
        None
    } else {
        Some(write_handoff(
            &cwd,
            source_kind,
            &target.label,
            &source_pane,
            &recent_output,
            context_tokens,
        )?)
    };

    let split_response = request(Method::PaneSplit(PaneSplitParams {
        workspace_id: Some(workspace_id),
        target_pane_id: Some(source_pane),
        direction: SplitDirection::Right,
        ratio: None,
        cwd: Some(cwd.to_string_lossy().into_owned()),
        focus: true,
        right_click: Default::default(),
        env: target.env,
    }))?;
    let new_pane = string_field(result_field(&split_response, "pane")?, "pane_id")?;
    let agent_name = new_agent_name(&target.label);

    request(Method::AgentStart(AgentStartParams {
        name: agent_name.clone(),
        kind: target.kind,
        pane_id: new_pane.clone(),
        command: Some(target.executable),
        args: target.args,
        timeout_ms: Some(timeout_ms),
    }))?;

    if let Some(path) = handoff_path.as_ref() {
        let prompt = format!(
            "Continue the existing task. Read the deterministic handoff packet at `{}` first, then inspect the workspace and git diff. Treat the packet as context, not proof. Continue with the next unfinished task without restating the entire history.",
            path.display()
        );
        request(Method::AgentPrompt(AgentPromptParams {
            target: agent_name.clone(),
            text: prompt,
            wait: Some(AgentPromptWaitOptions {
                until: vec![AgentStatus::Idle, AgentStatus::Done, AgentStatus::Blocked],
                timeout_ms: Some(timeout_ms),
            }),
        }))?;
    }

    request(Method::PaneFocus(PaneTarget {
        pane_id: new_pane.clone(),
    }))?;

    println!(
        "{}",
        serde_json::json!({
            "switched": true,
            "from": source,
            "to": target.label,
            "agent": agent_name,
            "pane_id": new_pane,
            "handoff": handoff_path,
            "context_tokens": context_tokens,
        })
    );
    Ok(0)
}

fn request(method: Method) -> io::Result<Value> {
    let response = super::send_request(&Request {
        id: format!("cli:switch:{}", timestamp_millis()),
        method,
    })?;
    if let Some(error) = response.get("error") {
        eprintln!("{error}");
        return Err(io::Error::other("switch API request failed"));
    }
    Ok(response)
}

fn result_field<'a>(response: &'a Value, field: &str) -> io::Result<&'a Value> {
    response
        .get("result")
        .and_then(|result| result.get(field))
        .ok_or_else(|| io::Error::other(format!("switch response missing result.{field}")))
}

fn string_field(value: &Value, field: &str) -> io::Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other(format!("switch response missing {field}")))
}

fn read_recent(target: &str) -> io::Result<String> {
    let response = request(Method::AgentRead(AgentReadParams {
        target: target.to_owned(),
        source: ReadSource::RecentUnwrapped,
        lines: Some(120),
        format: ReadFormat::Text,
        strip_ansi: true,
    }))?;
    Ok(result_field(&response, "read")?
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned())
}

fn write_handoff(
    cwd: &Path,
    source: &str,
    target: &str,
    source_pane: &str,
    recent_output: &str,
    max_tokens: usize,
) -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let directory = home.join(".config/innov/handoffs");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}-to-{}.md", timestamp_millis(), target));

    let status = git_output(cwd, &["status", "--short"]);
    let diff_stat = git_output(cwd, &["diff", "--stat"]);
    let budget = max_tokens.max(1).saturating_mul(CONTEXT_CHARS_PER_TOKEN);
    let mut packet = String::new();
    append_budgeted(
        &mut packet,
        budget,
        &format!(
            "# CLI Agent Handoff\n\nTreat the workspace and git state as the source of truth. This packet is bounded context, not proof.\n\n## Session\n- Source agent: `{source}`\n- Target backend: `{target}`\n- Source pane: `{source_pane}`\n- Workspace: `{}`\n\n",
            cwd.display()
        ),
    );
    append_budgeted(
        &mut packet,
        budget,
        &format!("## Git Status\n```text\n{status}\n```\n\n"),
    );
    append_budgeted(
        &mut packet,
        budget,
        &format!("## Diff Summary\n```text\n{diff_stat}\n```\n\n"),
    );
    append_budgeted(
        &mut packet,
        budget,
        &format!(
            "## Recent Terminal Context\n```text\n{}\n```\n\n",
            recent_output.replace('\r', "")
        ),
    );
    append_budgeted(
        &mut packet,
        budget,
        "## Next Action\nInspect the workspace and diff, then continue the unfinished task.\n",
    );
    std::fs::write(&path, packet)?;
    Ok(path)
}

fn append_budgeted(output: &mut String, budget: usize, block: &str) {
    let remaining = budget.saturating_sub(output.chars().count());
    if remaining == 0 {
        return;
    }
    if block.chars().count() <= remaining {
        output.push_str(block);
        return;
    }

    const TRUNCATION_MARKER: &str = "\n[truncated]";
    if remaining <= TRUNCATION_MARKER.chars().count() {
        output.extend(block.chars().take(remaining));
        return;
    }
    output.extend(
        block
            .chars()
            .take(remaining - TRUNCATION_MARKER.chars().count()),
    );
    output.push_str(TRUNCATION_MARKER);
}

fn git_output(cwd: &Path, arguments: &[&str]) -> String {
    match Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Ok(output) => format!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => format!("git command unavailable: {error}"),
    }
}

fn new_agent_name(backend: &str) -> String {
    format!("cli-{backend}-{}", timestamp_millis())
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn print_switch_help() {
    eprintln!("innov switch commands:");
    eprintln!("  innov switch <agent-target> --to <codex|claude|agy|profile-alias> [OPTIONS]");
    eprintln!("    --context-tokens N   bound the handoff packet (default: 3500)");
    eprintln!("    --no-handoff         start the backend without sending context");
    eprintln!("    --timeout MS         backend startup/prompt timeout (default: 120000)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_switch_options_with_defaults() {
        assert_eq!(
            parse_switch_options(&args(&["agent-1", "--to", "codex"])),
            Ok(Some(SwitchOptions {
                source: "agent-1".into(),
                backend: "codex".into(),
                no_handoff: false,
                timeout_ms: DEFAULT_TIMEOUT_MS,
                context_tokens: DEFAULT_CONTEXT_TOKENS,
            }))
        );
    }

    #[test]
    fn parses_switch_options_with_context_controls() {
        let parsed = parse_switch_options(&args(&[
            "agent-1",
            "--to",
            "agy",
            "--no-handoff",
            "--timeout",
            "3001",
            "--context-tokens",
            "900",
        ]));

        assert_eq!(
            parsed,
            Ok(Some(SwitchOptions {
                source: "agent-1".into(),
                backend: "agy".into(),
                no_handoff: true,
                timeout_ms: 3001,
                context_tokens: 900,
            }))
        );
    }

    #[test]
    fn rejects_unsupported_backend_and_invalid_budget() {
        assert_eq!(
            parse_switch_options(&args(&["agent-1", "--to", "luna-max"])),
            Ok(Some(SwitchOptions {
                source: "agent-1".into(),
                backend: "luna-max".into(),
                no_handoff: false,
                timeout_ms: DEFAULT_TIMEOUT_MS,
                context_tokens: DEFAULT_CONTEXT_TOKENS,
            }))
        );
        assert!(parse_switch_options(&args(&["agent-1", "--to", "two words"])).is_err());
        assert!(parse_switch_options(&args(&[
            "agent-1",
            "--to",
            "codex",
            "--context-tokens",
            "0",
        ]))
        .is_err());
    }

    #[test]
    fn handoff_budget_is_a_hard_character_limit() {
        let mut packet = String::new();
        append_budgeted(&mut packet, 16, "0123456789abcdefghijklmnopqrstuvwxyz");
        assert!(packet.chars().count() <= 16);
        assert!(packet.ends_with("[truncated]") || packet == "0123456789abcdef");
    }
}
