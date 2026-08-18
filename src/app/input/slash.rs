use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use std::process::{Command, Stdio};

use crate::input::TerminalKey;

const SWITCH_PREFIX: &str = "/switch";
const MAX_COMMAND_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashCommandAction {
    PassThrough,
    Consumed,
    Execute(String),
    Forward(String),
    ForwardAndContinue(String),
}

#[derive(Debug, Default)]
pub(crate) struct SlashCommandBuffer {
    text: String,
}

impl SlashCommandBuffer {
    pub(crate) fn is_active(&self) -> bool {
        !self.text.is_empty()
    }

    pub(crate) fn handle_key(&mut self, key: &TerminalKey) -> SlashCommandAction {
        if key.kind != KeyEventKind::Press {
            return if self.is_active() {
                SlashCommandAction::Consumed
            } else {
                SlashCommandAction::PassThrough
            };
        }

        let generated_text = key.generated_text.clone().or_else(|| {
            matches!(key.code, KeyCode::Char(ch) if !ch.is_control()
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT))
            .then(|| match key.code {
                KeyCode::Char(ch) => ch.to_string(),
                _ => unreachable!("character match must produce a character"),
            })
        });
        if let Some(text) = generated_text.filter(|text| !text.is_empty()) {
            return self.handle_text(&text);
        }

        match key.code {
            KeyCode::Backspace if self.is_active() => {
                self.text.pop();
                SlashCommandAction::Consumed
            }
            KeyCode::Enter if self.is_active() => self.handle_enter(),
            KeyCode::Esc if self.is_active() => {
                self.text.clear();
                SlashCommandAction::Consumed
            }
            _ if self.is_active() => {
                let text = std::mem::take(&mut self.text);
                SlashCommandAction::ForwardAndContinue(text)
            }
            _ => SlashCommandAction::PassThrough,
        }
    }

    pub(crate) fn handle_text(&mut self, text: &str) -> SlashCommandAction {
        if text.is_empty() {
            return SlashCommandAction::PassThrough;
        }

        if self.text.is_empty() && !text.starts_with('/') {
            return SlashCommandAction::PassThrough;
        }

        let mut candidate = std::mem::take(&mut self.text);
        candidate.push_str(text);

        if candidate.chars().count() > MAX_COMMAND_CHARS {
            return SlashCommandAction::Forward(candidate);
        }

        if candidate.contains(['\r', '\n']) {
            return self.finish_text(candidate);
        }

        if is_possible_switch_prefix(&candidate) {
            self.text = candidate;
            SlashCommandAction::Consumed
        } else {
            SlashCommandAction::Forward(candidate)
        }
    }

    fn handle_enter(&mut self) -> SlashCommandAction {
        let text = std::mem::take(&mut self.text);
        match parse_switch_command(&text) {
            Some(backend) => SlashCommandAction::Execute(backend),
            None => SlashCommandAction::Forward(format!("{text}\r")),
        }
    }

    fn finish_text(&mut self, text: String) -> SlashCommandAction {
        let line = text.strip_suffix('\n').or_else(|| text.strip_suffix('\r'));
        if let Some(line) = line.filter(|line| !line.contains(['\r', '\n'])) {
            if let Some(backend) = parse_switch_command(line) {
                self.text.clear();
                return SlashCommandAction::Execute(backend);
            }
        }
        SlashCommandAction::Forward(text)
    }
}

pub(crate) fn parse_switch_command(text: &str) -> Option<String> {
    let mut parts = text.split(' ');
    if parts.next()? != SWITCH_PREFIX {
        return None;
    }
    let backend = parts.next()?;
    if parts.next().is_some() || !is_valid_backend_name(backend) {
        return None;
    }
    Some(backend.to_owned())
}

fn is_possible_switch_prefix(text: &str) -> bool {
    if SWITCH_PREFIX.starts_with(text) {
        return true;
    }

    let Some(backend) = text.strip_prefix("/switch ") else {
        return false;
    };
    backend.is_empty() || is_valid_backend_prefix(backend)
}

fn is_valid_backend_prefix(value: &str) -> bool {
    value.chars().enumerate().all(|(index, ch)| {
        (index == 0 && (ch.is_ascii_alphabetic() || ch == '_'))
            || (index > 0 && (ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
    })
}

fn is_valid_backend_name(value: &str) -> bool {
    !value.is_empty() && is_valid_backend_prefix(value)
}

impl super::super::App {
    pub(super) fn launch_switch_from_slash(&mut self, backend: &str) {
        if let Err(error) = crate::cli::validate_switch_backend(backend) {
            self.show_switch_toast("switch failed", &error.to_string());
            return;
        }

        let Some(ws_idx) = self.state.active else {
            self.show_switch_toast("switch failed", "no active workspace");
            return;
        };
        let Some(pane_id) = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|workspace| workspace.focused_pane_id())
        else {
            self.show_switch_toast("switch failed", "no focused pane");
            return;
        };
        let Some(agent) = self.agent_info(ws_idx, pane_id) else {
            self.show_switch_toast("switch failed", "the focused pane is not an agent");
            return;
        };
        let source = agent.name.unwrap_or(agent.pane_id);
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                self.show_switch_toast("switch failed", &error.to_string());
                return;
            }
        };

        let mut command = Command::new(executable);
        command
            .args(["switch", source.as_str(), "--to", backend])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::configure_background_command(&mut command);

        match command.spawn() {
            Ok(child) => {
                self.detached_process_children.push(child);
                self.show_switch_toast("switching agent", &format!("{source} -> {backend}"));
            }
            Err(error) => self.show_switch_toast("switch failed", &error.to_string()),
        }
    }

    fn show_switch_toast(&mut self, title: &str, context: &str) {
        let previous = self.state.toast.clone();
        self.state.toast = Some(crate::app::state::ToastNotification {
            kind: crate::app::state::ToastKind::UpdateInstalled,
            title: title.to_owned(),
            context: context.to_owned(),
            position: None,
            target: None,
        });
        self.sync_toast_deadline(previous);
        self.render_notify.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(ch: char) -> TerminalKey {
        TerminalKey::new(KeyCode::Char(ch), KeyModifiers::empty()).with_text_commit()
    }

    fn type_text(buffer: &mut SlashCommandBuffer, text: &str) -> SlashCommandAction {
        let mut action = SlashCommandAction::PassThrough;
        for ch in text.chars() {
            action = buffer.handle_key(&key(ch));
            assert!(!matches!(action, SlashCommandAction::Forward(_)));
        }
        action
    }

    #[test]
    fn parses_supported_switch_forms() {
        assert_eq!(parse_switch_command("/switch agy"), Some("agy".into()));
        assert_eq!(parse_switch_command("/switch co-ht"), Some("co-ht".into()));
    }

    #[test]
    fn rejects_malformed_switch_commands() {
        assert_eq!(parse_switch_command("/switch"), None);
        assert_eq!(parse_switch_command("/switch co ht"), None);
        assert_eq!(parse_switch_command("/switch 1profile"), None);
    }

    #[test]
    fn buffers_switch_until_enter() {
        let mut buffer = SlashCommandBuffer::default();
        assert_eq!(
            type_text(&mut buffer, "/switch agy"),
            SlashCommandAction::Consumed
        );
        assert!(buffer.is_active());
        assert_eq!(
            buffer.handle_key(&TerminalKey::new(KeyCode::Enter, KeyModifiers::empty())),
            SlashCommandAction::Execute("agy".into())
        );
        assert!(!buffer.is_active());
    }

    #[test]
    fn pasted_switch_line_executes() {
        let mut buffer = SlashCommandBuffer::default();
        assert_eq!(
            buffer.handle_text("/switch co-ht\n"),
            SlashCommandAction::Execute("co-ht".into())
        );
        assert!(!buffer.is_active());
    }

    #[test]
    fn non_switch_slash_input_is_forwarded_without_loss() {
        let mut buffer = SlashCommandBuffer::default();
        assert_eq!(buffer.handle_key(&key('/')), SlashCommandAction::Consumed);
        assert_eq!(
            buffer.handle_key(&key('h')),
            SlashCommandAction::Forward("/h".into())
        );
        assert!(!buffer.is_active());
    }

    #[test]
    fn enter_forwards_an_invalid_buffer_with_the_enter_key() {
        let mut buffer = SlashCommandBuffer::default();
        let _ = type_text(&mut buffer, "/switch ");
        assert_eq!(
            buffer.handle_key(&TerminalKey::new(KeyCode::Enter, KeyModifiers::empty())),
            SlashCommandAction::Forward("/switch \r".into())
        );
    }
}
