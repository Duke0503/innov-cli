//! Profile registry, native CLI config writers, and shell aliases.
//!
//! A profile is intent (provider, model, permissions, and credentials). The
//! writer turns that intent into the config format owned by the selected CLI;
//! the runtime can then launch the same profile without depending on a shell
//! alias being loaded in a non-interactive pane.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use toml_edit::{value, DocumentMut, Item, Table};

const REGISTRY_VERSION: u32 = 1;
const RC_MARKER: &str = "cli/profiles";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CliKind {
    Claude,
    Codex,
    Agy,
}

impl CliKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "agy" | "antigravity" => Some(Self::Agy),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
        }
    }

    fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DangerLevel {
    #[default]
    Ask,
    AcceptEdits,
    WorkspaceWrite,
    Bypass,
}

impl DangerLevel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(Self::Ask),
            "accept_edits" => Some(Self::AcceptEdits),
            "workspace_write" => Some(Self::WorkspaceWrite),
            "bypass" => Some(Self::Bypass),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AcceptEdits => "accept_edits",
            Self::WorkspaceWrite => "workspace_write",
            Self::Bypass => "bypass",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModelMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sonnet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub haiku: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Profile {
    pub alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    pub cli: CliKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    /// Inline keys are supported for compatibility, but env references are preferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub env_var: String,
    #[serde(default)]
    pub danger: DangerLevel,
    #[serde(default)]
    pub model_map: ModelMap,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_api: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Profile {
    fn cli_profile_name(&self) -> &str {
        self.profile_name.as_deref().unwrap_or(&self.alias)
    }

    fn executable(&self) -> &str {
        self.command
            .as_deref()
            .unwrap_or_else(|| self.cli.executable())
    }

    fn api_key(&self) -> io::Result<Option<String>> {
        match self.api_key_env.as_deref() {
            Some(name) => std::env::var(name).map(Some).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "profile {} expects key from ${name}, but it is not set",
                        self.alias
                    ),
                )
            }),
            None => Ok(self.api_key.clone()),
        }
    }

    fn config_path(&self, home: &Path) -> Option<PathBuf> {
        match self.cli {
            CliKind::Claude => Some(
                home.join(".claude")
                    .join("profiles")
                    .join(format!("{}.json", self.cli_profile_name())),
            ),
            CliKind::Codex => Some(
                home.join(".codex")
                    .join(format!("{}.config.toml", self.cli_profile_name())),
            ),
            CliKind::Agy => None,
        }
    }

    fn agent_args(&self, home: &Path) -> Vec<String> {
        let mut args = match (self.cli, self.config_path(home)) {
            (CliKind::Claude, Some(path)) => {
                vec!["--settings".into(), path.display().to_string()]
            }
            (CliKind::Codex, Some(_)) => {
                vec!["--profile".into(), self.cli_profile_name().into()]
            }
            (CliKind::Agy, None) => Vec::new(),
            (CliKind::Agy, Some(_)) => Vec::new(),
            (_, None) => Vec::new(),
        };
        args.extend(self.args.iter().cloned());
        args
    }

    fn launch_env(&self) -> io::Result<HashMap<String, String>> {
        let Some(key) = self.api_key()? else {
            return Ok(HashMap::new());
        };
        if self.env_var.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "profile {} has a key but no target environment variable",
                    self.alias
                ),
            ));
        }
        let mut env = HashMap::new();
        env.insert(self.env_var.clone(), key);
        Ok(env)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Registry {
    version: u32,
    #[serde(default)]
    profiles: Vec<Profile>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProfileRegistry {
    profiles: Vec<Profile>,
}

impl ProfileRegistry {
    fn load() -> io::Result<Self> {
        let path = registry_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Self { profiles: vec![] })
            }
            Err(err) => return Err(err),
        };
        let registry: Registry = toml::from_str(&text).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not valid profile TOML: {err}", path.display()),
            )
        })?;
        if registry.version != REGISTRY_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported profile registry version {}", registry.version),
            ));
        }
        validate_profiles(&registry.profiles)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(Self {
            profiles: registry.profiles,
        })
    }

    fn save(&self) -> io::Result<()> {
        validate_profiles(&self.profiles)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let path = registry_path();
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("profile path has no parent"))?;
        fs::create_dir_all(parent)?;
        let text = toml::to_string_pretty(&Registry {
            version: REGISTRY_VERSION,
            profiles: self.profiles.clone(),
        })
        .map_err(|err| io::Error::other(err.to_string()))?;
        write_private_file(&path, text.as_bytes())
    }

    fn find(&self, alias: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.alias == alias)
    }
}

pub(crate) struct PreparedProfile {
    pub kind: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub executable: String,
}

pub(crate) struct ResolvedTarget {
    pub label: String,
    pub kind: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub executable: String,
}

pub(crate) fn prepare_profile(profile: &Profile, home: &Path) -> io::Result<PreparedProfile> {
    write_profile_config(home, profile)?;
    Ok(PreparedProfile {
        kind: profile.cli.label().into(),
        args: profile.agent_args(home),
        env: profile.launch_env()?,
        executable: profile.executable().into(),
    })
}

pub(crate) fn resolve_switch_target(target: &str, home: &Path) -> io::Result<ResolvedTarget> {
    let registry = ProfileRegistry::load()?;
    if let Some(profile) = registry.find(target) {
        let prepared = prepare_profile(profile, home)?;
        return Ok(ResolvedTarget {
            label: target.into(),
            kind: prepared.kind,
            args: prepared.args,
            env: prepared.env,
            executable: prepared.executable,
        });
    }
    let kind = CliKind::parse(target).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown backend or profile alias {target:?}"),
        )
    })?;
    Ok(ResolvedTarget {
        label: target.into(),
        kind: kind.label().into(),
        args: Vec::new(),
        env: HashMap::new(),
        executable: kind.executable().into(),
    })
}

pub(crate) fn run_profile_command(args: &[String]) -> io::Result<i32> {
    let Some(command) = args.first().map(String::as_str) else {
        print_profile_help();
        return Ok(2);
    };
    match command {
        "list" => profile_list(&args[1..]),
        "add" => profile_add(&args[1..]),
        "remove" => profile_remove(&args[1..]),
        "install" => profile_install(&args[1..]),
        "exec" => profile_exec(&args[1..]),
        "path" => profile_path(&args[1..]),
        "help" | "--help" | "-h" => {
            print_profile_help();
            Ok(0)
        }
        _ => {
            print_profile_help();
            Ok(2)
        }
    }
}

fn profile_list(args: &[String]) -> io::Result<i32> {
    let json = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            eprintln!("usage: cli profile list [--json]");
            return Ok(2);
        }
    };
    let registry = ProfileRegistry::load()?;
    if json {
        let profiles: Vec<_> = registry.profiles.iter().map(profile_summary).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&profiles)
                .map_err(|err| io::Error::other(err.to_string()))?
        );
    } else if registry.profiles.is_empty() {
        println!("No profiles configured. Add one with `cli profile add`.");
    } else {
        for profile in &registry.profiles {
            let model = profile
                .model_map
                .default
                .as_deref()
                .or(profile.model_map.sonnet.as_deref())
                .unwrap_or("default");
            let key = if profile.api_key_env.is_some() {
                "env"
            } else if profile.api_key.is_some() {
                "stored"
            } else {
                "inherited"
            };
            println!(
                "{:<18} {:<7} {:<18} {:<16} model={} key={}",
                profile.alias,
                profile.cli.label(),
                profile.provider,
                profile.danger.label(),
                model,
                key
            );
        }
    }
    Ok(0)
}

fn profile_summary(profile: &Profile) -> serde_json::Value {
    serde_json::json!({
        "alias": profile.alias,
        "cli": profile.cli.label(),
        "command": profile.executable(),
        "profile_name": profile.cli_profile_name(),
        "provider": profile.provider,
        "base_url": profile.base_url,
        "env_var": profile.env_var,
        "api_key_source": profile.api_key_env.as_deref().map(|name| format!("env:{name}"))
            .or_else(|| profile.api_key.as_ref().map(|_| "stored".into()))
            .unwrap_or_else(|| "inherited".into()),
        "danger": profile.danger.label(),
        "model": profile.model_map.default.clone(),
        "args": profile.args.clone(),
    })
}

fn profile_add(args: &[String]) -> io::Result<i32> {
    let alias = args.first().cloned().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cli profile add <alias> --cli <kind> [OPTIONS]",
        )
    })?;
    let mut cli = None;
    let mut provider = None;
    let mut base_url = None;
    let mut profile_name = None;
    let mut command = None;
    let mut api_key = None;
    let mut api_key_env = None;
    let mut api_key_stdin = false;
    let mut env_var = None;
    let mut danger = DangerLevel::Ask;
    let mut model = None;
    let mut opus_model = None;
    let mut sonnet_model = None;
    let mut haiku_model = None;
    let mut wire_api = None;
    let mut profile_args = Vec::new();
    let mut replace = false;

    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--cli" => cli = Some(take_value(args, &mut index, flag)?),
            "--provider" => provider = Some(take_value(args, &mut index, flag)?),
            "--base-url" => base_url = Some(take_value(args, &mut index, flag)?),
            "--profile-name" => profile_name = Some(take_value(args, &mut index, flag)?),
            "--command" => command = Some(take_value(args, &mut index, flag)?),
            "--api-key" => api_key = Some(take_value(args, &mut index, flag)?),
            "--api-key-env" => api_key_env = Some(take_value(args, &mut index, flag)?),
            "--api-key-stdin" => {
                api_key_stdin = true;
                index += 1;
            }
            "--env-var" => env_var = Some(take_value(args, &mut index, flag)?),
            "--danger" => {
                let value = take_value(args, &mut index, flag)?;
                danger = DangerLevel::parse(&value).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("invalid --danger value {value:?}"),
                    )
                })?;
            }
            "--model" => model = Some(take_value(args, &mut index, flag)?),
            "--opus-model" => opus_model = Some(take_value(args, &mut index, flag)?),
            "--sonnet-model" => sonnet_model = Some(take_value(args, &mut index, flag)?),
            "--haiku-model" => haiku_model = Some(take_value(args, &mut index, flag)?),
            "--wire-api" => wire_api = Some(take_value(args, &mut index, flag)?),
            "--arg" => profile_args.push(take_any_value(args, &mut index, flag)?),
            "--replace" => {
                replace = true;
                index += 1;
            }
            "--help" | "-h" => {
                print_profile_add_help();
                return Ok(0);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown profile add option {other}"),
                ));
            }
        }
    }

    let cli_name =
        cli.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing required --cli"))?;
    let kind = CliKind::parse(&cli_name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cli must be codex, claude, or agy",
        )
    })?;
    if api_key.is_some() as u8 + api_key_env.is_some() as u8 + api_key_stdin as u8 > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "choose only one of --api-key, --api-key-env, or --api-key-stdin",
        ));
    }
    if api_key_stdin {
        let mut secret = String::new();
        io::stdin().read_to_string(&mut secret)?;
        api_key = Some(secret.trim_end_matches(['\r', '\n']).to_owned());
    }
    if api_key.as_deref() == Some("") {
        api_key = None;
    }

    let profile_name = profile_name.or_else(|| Some(alias.clone()));
    let provider = provider.unwrap_or_else(|| kind.label().into());
    let base_url = base_url.unwrap_or_else(|| match kind {
        CliKind::Codex => "https://api.openai.com/v1".into(),
        CliKind::Claude => "https://api.anthropic.com".into(),
        CliKind::Agy => String::new(),
    });
    let env_var = env_var.unwrap_or_else(|| match kind {
        CliKind::Claude => "ANTHROPIC_AUTH_TOKEN".into(),
        _ => format!("CLI_{}_API_KEY", env_name(&alias)),
    });
    let mut model_map = ModelMap {
        default: model.clone(),
        opus: opus_model,
        sonnet: sonnet_model,
        haiku: haiku_model,
    };
    if kind == CliKind::Claude {
        if let Some(model) = model {
            model_map.opus.get_or_insert_with(|| model.clone());
            model_map.sonnet.get_or_insert_with(|| model.clone());
            model_map.haiku.get_or_insert(model);
        }
    }
    let profile = Profile {
        alias: alias.clone(),
        profile_name,
        cli: kind,
        command,
        provider,
        base_url,
        api_key,
        api_key_env,
        env_var,
        danger,
        model_map,
        wire_api,
        args: profile_args,
    };
    validate_profile(&profile).map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    let mut registry = ProfileRegistry::load()?;
    if let Some(existing) = registry.profiles.iter().position(|p| p.alias == alias) {
        if !replace {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("profile {alias:?} already exists; pass --replace to update it"),
            ));
        }
        registry.profiles[existing] = profile;
    } else {
        registry.profiles.push(profile);
    }
    registry.save()?;
    refresh_script_if_installed(&registry.profiles)?;
    println!("profile {alias} saved in {}", registry_path().display());
    Ok(0)
}

fn profile_remove(args: &[String]) -> io::Result<i32> {
    let [alias] = args else {
        eprintln!("usage: cli profile remove <alias>");
        return Ok(2);
    };
    let mut registry = ProfileRegistry::load()?;
    let before = registry.profiles.len();
    registry.profiles.retain(|profile| profile.alias != *alias);
    if registry.profiles.len() == before {
        eprintln!("profile {alias:?} does not exist");
        return Ok(1);
    }
    registry.save()?;
    refresh_script_if_installed(&registry.profiles)?;
    println!("profile {alias} removed");
    Ok(0)
}

fn profile_install(args: &[String]) -> io::Result<i32> {
    let mut shell = None;
    let mut rc = None;
    let mut no_rc = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--shell" => shell = Some(take_value(args, &mut index, "--shell")?),
            "--rc" => rc = Some(PathBuf::from(take_value(args, &mut index, "--rc")?)),
            "--no-rc" => {
                no_rc = true;
                index += 1;
            }
            "--help" | "-h" => {
                print_profile_install_help();
                return Ok(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    let registry = ProfileRegistry::load()?;
    let home = home_dir();
    for profile in &registry.profiles {
        if let Some(path) = write_profile_config(&home, profile)? {
            println!("wrote {}", path.display());
        }
    }
    let shell = parse_shell(shell.as_deref())?;
    let script_path = shell_script_path(shell);
    write_shell_script(&script_path, &home, &registry.profiles, shell)?;
    println!("wrote {}", script_path.display());

    if !no_rc {
        let rc_path = rc.unwrap_or_else(|| default_rc_path(shell, &home));
        let added = ensure_rc_line(&rc_path, &script_path, shell)?;
        println!(
            "{} {}",
            match added {
                RcOutcome::Added => "added source line to",
                RcOutcome::AlreadyPresent => "source line already present in",
            },
            rc_path.display()
        );
    }
    Ok(0)
}

fn profile_exec(args: &[String]) -> io::Result<i32> {
    let Some(alias) = args.first() else {
        eprintln!("usage: cli profile exec <alias> -- [ARGS...]");
        return Ok(2);
    };
    let separator = args.iter().position(|arg| arg == "--");
    if separator.is_none() && args.len() > 1 {
        eprintln!("usage: cli profile exec <alias> -- [ARGS...]");
        return Ok(2);
    }
    let extra = separator
        .map(|index| args[index + 1..].to_vec())
        .unwrap_or_default();
    let registry = ProfileRegistry::load()?;
    let profile = registry.find(alias).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("profile {alias:?} does not exist"),
        )
    })?;
    let home = home_dir();
    write_profile_config(&home, profile)?;
    let mut command = Command::new(profile.executable());
    command.args(profile.agent_args(&home));
    command.args(extra);
    for (name, value) in profile.launch_env()? {
        command.env(name, value);
    }
    let status = command.status().map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "cannot run profile {alias:?} ({:?}): {err}",
                profile.executable()
            ),
        )
    })?;
    Ok(status.code().unwrap_or(1))
}

fn profile_path(args: &[String]) -> io::Result<i32> {
    if !args.is_empty() {
        eprintln!("usage: cli profile path");
        return Ok(2);
    }
    println!("{}", registry_path().display());
    Ok(0)
}

fn refresh_script_if_installed(profiles: &[Profile]) -> io::Result<()> {
    let installed = [ShellKind::Posix, ShellKind::PowerShell]
        .into_iter()
        .filter_map(|shell| {
            let path = shell_script_path(shell);
            path.exists().then_some((shell, path))
        })
        .collect::<Vec<_>>();
    if installed.is_empty() {
        return Ok(());
    }
    let home = home_dir();
    for profile in profiles {
        write_profile_config(&home, profile)?;
    }
    for (shell, path) in installed {
        write_shell_script(&path, &home, profiles, shell)?;
    }
    Ok(())
}

fn write_profile_config(home: &Path, profile: &Profile) -> io::Result<Option<PathBuf>> {
    let Some(path) = profile.config_path(home) else {
        return Ok(None);
    };
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("profile config has no parent"))?;
    fs::create_dir_all(parent)?;
    let rendered = match profile.cli {
        CliKind::Claude => {
            let existing = match fs::read_to_string(&path) {
                Ok(text) => Some(serde_json::from_str::<serde_json::Value>(&text).map_err(
                    |err| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("{} is not valid JSON: {err}", path.display()),
                        )
                    },
                )?),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => return Err(err),
            };
            serde_json::to_string_pretty(&render_claude(profile, existing.as_ref()))
                .map_err(|err| io::Error::other(err.to_string()))?
                + "\n"
        }
        CliKind::Codex => render_codex(profile, fs::read_to_string(&path).ok().as_deref())?,
        CliKind::Agy => unreachable!(),
    };
    write_private_file(&path, rendered.as_bytes())?;
    Ok(Some(path))
}

fn render_claude(profile: &Profile, existing: Option<&serde_json::Value>) -> serde_json::Value {
    let mut root = match existing {
        Some(serde_json::Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    let mut env = match root.get("env") {
        Some(serde_json::Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    if profile.base_url.is_empty() {
        env.remove("ANTHROPIC_BASE_URL");
    } else {
        env.insert(
            "ANTHROPIC_BASE_URL".into(),
            serde_json::json!(profile.base_url),
        );
    }
    // Credentials stay in the command-scoped environment, not in settings JSON.
    env.remove("ANTHROPIC_AUTH_TOKEN");
    for (name, value) in [
        (
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            profile.model_map.opus.as_ref(),
        ),
        (
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            profile.model_map.sonnet.as_ref(),
        ),
        (
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            profile.model_map.haiku.as_ref(),
        ),
    ] {
        if let Some(value) = value {
            env.insert(name.into(), serde_json::json!(value));
        } else {
            env.remove(name);
        }
    }
    root.insert("env".into(), serde_json::Value::Object(env));

    let mut permissions = match root.get("permissions") {
        Some(serde_json::Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    permissions.insert(
        "defaultMode".into(),
        serde_json::json!(match profile.danger {
            DangerLevel::Ask => "default",
            DangerLevel::AcceptEdits | DangerLevel::WorkspaceWrite => "acceptEdits",
            DangerLevel::Bypass => "bypassPermissions",
        }),
    );
    root.insert("permissions".into(), serde_json::Value::Object(permissions));
    root.remove("defaultMode");
    if profile.danger == DangerLevel::Bypass {
        root.insert(
            "skipDangerousModePermissionPrompt".into(),
            serde_json::json!(true),
        );
    } else {
        root.remove("skipDangerousModePermissionPrompt");
    }
    serde_json::Value::Object(root)
}

fn provider_key(profile_name: &str) -> String {
    let sanitized: String = profile_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    format!("cli_{sanitized}")
}

fn render_codex(profile: &Profile, existing: Option<&str>) -> io::Result<String> {
    let mut document: DocumentMut = match existing {
        Some(text) => text.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("existing Codex config is invalid TOML: {err}"),
            )
        })?,
        None => DocumentMut::new(),
    };
    let key = provider_key(profile.cli_profile_name());
    document["model_provider"] = value(key.clone());
    if let Some(model) = &profile.model_map.default {
        document["model"] = value(model.clone());
    }
    let (approval, sandbox) = match profile.danger {
        DangerLevel::Ask => ("untrusted", "read-only"),
        DangerLevel::AcceptEdits => ("on-request", "workspace-write"),
        DangerLevel::WorkspaceWrite => ("never", "workspace-write"),
        DangerLevel::Bypass => ("never", "danger-full-access"),
    };
    document["approval_policy"] = value(approval);
    document["sandbox_mode"] = value(sandbox);
    if !document.contains_key("model_providers") {
        let mut table = Table::new();
        table.set_implicit(true);
        document["model_providers"] = Item::Table(table);
    }
    let providers = document["model_providers"].as_table_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex model_providers is not a table",
        )
    })?;
    let mut provider = Table::new();
    provider["name"] = value(profile.provider.clone());
    provider["base_url"] = value(profile.base_url.clone());
    provider["env_key"] = value(profile.env_var.clone());
    provider["wire_api"] = value(
        profile
            .wire_api
            .clone()
            .unwrap_or_else(|| "responses".into()),
    );
    providers[&key] = Item::Table(provider);
    Ok(document.to_string())
}

#[derive(Debug, Clone, Copy)]
enum ShellKind {
    Posix,
    PowerShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RcOutcome {
    Added,
    AlreadyPresent,
}

fn parse_shell(value: Option<&str>) -> io::Result<ShellKind> {
    match value {
        Some("posix") => Ok(ShellKind::Posix),
        Some("powershell") => Ok(ShellKind::PowerShell),
        None => Ok(detect_shell()),
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported shell {other:?}"),
        )),
    }
}

fn detect_shell() -> ShellKind {
    if cfg!(windows)
        || std::env::var("PSModulePath").is_ok()
        || std::env::var("SHELL")
            .map(|shell| shell.ends_with("pwsh") || shell.ends_with("powershell"))
            .unwrap_or(false)
    {
        ShellKind::PowerShell
    } else {
        ShellKind::Posix
    }
}

fn shell_script_path(shell: ShellKind) -> PathBuf {
    crate::config::config_dir().join(match shell {
        ShellKind::Posix => "profiles.sh",
        ShellKind::PowerShell => "profiles.ps1",
    })
}

fn default_rc_path(shell: ShellKind, home: &Path) -> PathBuf {
    match shell {
        ShellKind::Posix => {
            let shell = std::env::var("SHELL").unwrap_or_default();
            if shell.ends_with("zsh") {
                home.join(".zshrc")
            } else {
                home.join(".bashrc")
            }
        }
        ShellKind::PowerShell => {
            if let Ok(profile) = std::env::var("PROFILE") {
                PathBuf::from(profile)
            } else {
                home.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1")
            }
        }
    }
}

fn write_shell_script(
    path: &Path,
    home: &Path,
    profiles: &[Profile],
    shell: ShellKind,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("shell script has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut output = String::new();
    output.push_str(match shell {
        ShellKind::Posix => "# GENERATED BY cli - DO NOT EDIT\n# Changes are overwritten by `cli profile install`.\n\n",
        ShellKind::PowerShell => "# GENERATED BY cli - DO NOT EDIT\n# Changes are overwritten by `cli profile install`.\n\n",
    });
    for profile in profiles {
        output.push_str(&render_shell_profile(home, profile, shell));
    }
    write_private_file(path, output.as_bytes())
}

fn render_shell_profile(home: &Path, profile: &Profile, shell: ShellKind) -> String {
    let command = shell_command_token(profile.executable());
    let mut args = profile.agent_args(home);
    let rendered_args = match shell {
        ShellKind::Posix => args
            .drain(..)
            .map(|arg| shell_arg(&arg))
            .collect::<Vec<_>>()
            .join(" "),
        ShellKind::PowerShell => args
            .drain(..)
            .map(|arg| ps_arg(&arg))
            .collect::<Vec<_>>()
            .join(" "),
    };
    let (env_assignment, has_key) = match shell {
        ShellKind::Posix => {
            let assignment = match (&profile.api_key, &profile.api_key_env) {
                (Some(key), _) => format!("{}={} ", profile.env_var, shell_quote(key)),
                (None, Some(source)) => {
                    format!("{}=\"${{{source}}}\" ", profile.env_var)
                }
                (None, None) => String::new(),
            };
            (
                assignment,
                profile.api_key.is_some() || profile.api_key_env.is_some(),
            )
        }
        ShellKind::PowerShell => {
            let assignment = match (&profile.api_key, &profile.api_key_env) {
                (Some(key), _) => format!("  $env:{} = {}\n", profile.env_var, ps_quote(key)),
                (None, Some(source)) => {
                    format!("  $env:{} = $env:{}\n", profile.env_var, source)
                }
                (None, None) => String::new(),
            };
            (
                assignment,
                profile.api_key.is_some() || profile.api_key_env.is_some(),
            )
        }
    };
    match shell {
        ShellKind::Posix => {
            let invocation = if rendered_args.is_empty() {
                format!("{command} \"$@\"")
            } else {
                format!("{command} {rendered_args} \"$@\"")
            };
            format!(
                "# {} -> {}\n{}() {{\n  {}{}\n}}\n\n",
                profile.alias, profile.provider, profile.alias, env_assignment, invocation,
            )
        }
        ShellKind::PowerShell => {
            let invoke = format!("  & {} {} @args\n", command, rendered_args);
            if has_key {
                format!(
                    "# {} -> {}\nfunction {} {{\n  $cliPrevious = $env:{}\n{}  try {{\n{}  }} finally {{\n    $env:{} = $cliPrevious\n  }}\n}}\n\n",
                    profile.alias,
                    profile.provider,
                    profile.alias,
                    profile.env_var,
                    env_assignment,
                    invoke,
                    profile.env_var,
                )
            } else {
                format!(
                    "# {} -> {}\nfunction {} {{\n{}  & {} {} @args\n}}\n\n",
                    profile.alias, profile.provider, profile.alias, "  ", command, rendered_args
                )
            }
        }
    }
}

fn shell_command_token(command: &str) -> String {
    if command
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
    {
        command.into()
    } else {
        shell_quote(command)
    }
}

fn shell_arg(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '@' | '%' | '_' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
                )
        })
    {
        value.into()
    } else {
        shell_quote(value)
    }
}

fn ps_arg(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '@' | '%' | '_' | '+' | '=' | ':' | ',' | '.' | '/' | '-'
                )
        })
    {
        value.into()
    } else {
        ps_quote(value)
    }
}

fn ensure_rc_line(rc_path: &Path, script_path: &Path, shell: ShellKind) -> io::Result<RcOutcome> {
    let existing = fs::read_to_string(rc_path).unwrap_or_default();
    let script_marker = script_path.display().to_string().replace('\\', "/");
    if existing
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| {
            let line = line.replace('\\', "/");
            line.contains(RC_MARKER) || line.contains(&script_marker)
        })
    {
        return Ok(RcOutcome::AlreadyPresent);
    }
    let mut output = existing;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("\n# cli profiles\n");
    match shell {
        ShellKind::Posix => {
            let path = shell_quote(&script_path.display().to_string());
            output.push_str(&format!("[ -f {path} ] && . {path} # {RC_MARKER}\n"));
        }
        ShellKind::PowerShell => {
            let path = ps_quote(&script_path.display().to_string());
            output.push_str(&format!(
                "if (Test-Path {path}) {{ . {path} }} # {RC_MARKER}\n"
            ));
        }
    }
    write_private_file(rc_path, output.as_bytes())?;
    Ok(RcOutcome::Added)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> io::Result<String> {
    let value = args
        .get(*index + 1)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{flag} requires a value"),
            )
        })?;
    *index += 2;
    Ok(value.clone())
}

fn take_any_value(args: &[String], index: &mut usize, flag: &str) -> io::Result<String> {
    let value = args.get(*index + 1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{flag} requires a value"),
        )
    })?;
    *index += 2;
    Ok(value.clone())
}

fn validate_profiles(profiles: &[Profile]) -> Result<(), String> {
    let mut aliases = HashSet::new();
    for profile in profiles {
        validate_profile(profile)?;
        if !aliases.insert(profile.alias.as_str()) {
            return Err(format!("duplicate profile alias {:?}", profile.alias));
        }
    }
    Ok(())
}

fn validate_profile(profile: &Profile) -> Result<(), String> {
    validate_shell_name(&profile.alias, "alias")?;
    if let Some(name) = &profile.profile_name {
        validate_shell_name(name, "profile name")?;
    }
    if let Some(command) = &profile.command {
        if command.is_empty()
            || command
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err("command must be a single executable or shell alias".into());
        }
    }
    validate_env_name(&profile.env_var)?;
    if profile.cli == CliKind::Claude && profile.env_var != "ANTHROPIC_AUTH_TOKEN" {
        return Err("Claude profiles must use env_var=ANTHROPIC_AUTH_TOKEN".into());
    }
    if let Some(name) = &profile.api_key_env {
        validate_env_name(name)?;
    }
    if profile.api_key.is_some() && profile.api_key_env.is_some() {
        return Err("choose one credential source: api_key or api_key_env".into());
    }
    if profile
        .api_key
        .as_ref()
        .is_some_and(|key| key.contains('\0') || key.contains('\n') || key.contains('\r'))
    {
        return Err("api key must not contain NUL or newline characters".into());
    }
    if profile.base_url.chars().any(char::is_control)
        || profile.provider.chars().any(char::is_control)
    {
        return Err("provider and base URL must not contain control characters".into());
    }
    if profile
        .args
        .iter()
        .any(|arg| arg.chars().any(char::is_control))
    {
        return Err("profile arguments must not contain control characters".into());
    }
    Ok(())
}

fn validate_shell_name(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(format!("{field} must start with a letter or underscore and contain only letters, digits, _ or -"));
    }
    Ok(())
}

fn validate_env_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(format!("invalid environment variable name {value:?}"));
    }
    Ok(())
}

fn env_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn registry_path() -> PathBuf {
    crate::config::config_dir().join("profiles.toml")
}

fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn print_profile_help() {
    eprintln!("cli profile commands:");
    eprintln!("  cli profile list [--json]");
    eprintln!("  cli profile add <alias> --cli <codex|claude|agy> [OPTIONS]");
    eprintln!("  cli profile remove <alias>");
    eprintln!("  cli profile install [--shell posix|powershell] [--rc PATH] [--no-rc]");
    eprintln!("  cli profile exec <alias> -- [ARGS...]");
    eprintln!("  cli profile path");
}

fn print_profile_add_help() {
    print_profile_help();
    eprintln!("  --api-key-env ENV     read the key from an existing environment variable");
    eprintln!("  --api-key-stdin       read the key from stdin without putting it in argv");
    eprintln!("  --danger LEVEL        ask|accept_edits|workspace_write|bypass");
    eprintln!("  --model MODEL         default model (Claude applies it to all roles)");
    eprintln!("  --replace             replace an existing alias");
}

fn print_profile_install_help() {
    eprintln!("usage: cli profile install [--shell posix|powershell] [--rc PATH] [--no-rc]");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(cli: CliKind) -> Profile {
        Profile {
            alias: "ht".into(),
            profile_name: Some("ht".into()),
            cli,
            command: None,
            provider: "gateway".into(),
            base_url: "https://gateway.test/v1".into(),
            api_key: Some("secret".into()),
            api_key_env: None,
            env_var: "CLI_HT_API_KEY".into(),
            danger: DangerLevel::Bypass,
            model_map: ModelMap {
                default: Some("luna-max".into()),
                opus: Some("luna-max".into()),
                sonnet: Some("luna-max".into()),
                haiku: Some("luna-max".into()),
            },
            wire_api: Some("responses".into()),
            args: vec![],
        }
    }

    #[test]
    fn codex_writer_maps_bypass_to_both_axes() {
        let text = render_codex(&profile(CliKind::Codex), None).unwrap();
        let parsed: toml::Value = text.parse().unwrap();
        assert_eq!(parsed["approval_policy"].as_str(), Some("never"));
        assert_eq!(parsed["sandbox_mode"].as_str(), Some("danger-full-access"));
        assert_eq!(parsed["model"].as_str(), Some("luna-max"));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn claude_writer_nests_permissions_and_does_not_persist_secret() {
        let rendered = render_claude(&profile(CliKind::Claude), None);
        assert_eq!(rendered["permissions"]["defaultMode"], "bypassPermissions");
        assert_eq!(
            rendered["env"]["ANTHROPIC_BASE_URL"],
            "https://gateway.test/v1"
        );
        assert!(rendered["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    }

    #[test]
    fn claude_writer_preserves_foreign_settings() {
        let existing = serde_json::json!({"model": "opus", "hooks": {"trusted": true}});
        let rendered = render_claude(&profile(CliKind::Claude), Some(&existing));
        assert_eq!(rendered["model"], "opus");
        assert_eq!(rendered["hooks"]["trusted"], true);
    }

    #[test]
    fn shell_function_forwards_arguments_and_scopes_key() {
        let home = Path::new("/home/dev");
        let script = render_shell_profile(home, &profile(CliKind::Codex), ShellKind::Posix);
        assert!(script.contains("CLI_HT_API_KEY='secret'"));
        assert!(script.contains("codex --profile ht \"$@\""));
        assert!(script.contains("ht()"));
        assert!(!script.contains("export CLI_HT_API_KEY"));
    }

    #[test]
    fn aliases_and_env_names_are_shell_safe() {
        assert!(validate_shell_name("cx-ht", "alias").is_ok());
        assert!(validate_shell_name("cx ht", "alias").is_err());
        assert!(validate_env_name("CLI_CX_HT_API_KEY").is_ok());
        assert!(validate_env_name("1KEY").is_err());
    }

    #[test]
    fn profile_summary_redacts_inline_key() {
        let summary = profile_summary(&profile(CliKind::Codex));
        assert!(summary.to_string().contains("stored"));
        assert!(!summary.to_string().contains("secret"));
    }
}
