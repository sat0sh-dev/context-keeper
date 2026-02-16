use serde::Deserialize;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;
use regex::Regex;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Deserialize, Default)]
struct Config {
    project: Option<ProjectConfig>,
    scripts: Option<ScriptsConfig>,
    containers: Option<ContainersConfig>,
    hints: Option<HintsConfig>,
    history: Option<HistoryConfig>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfig {
    name: Option<String>,
    #[serde(rename = "type")]
    project_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScriptsConfig {
    entry_point: Option<String>,
    config_dir: Option<String>,
    config_pattern: Option<String>,
    extract_vars: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ContainersConfig {
    runtime: Option<String>, // "podman" or "docker"
}

#[derive(Debug, Deserialize)]
struct HintsConfig {
    default: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryConfig {
    enabled: Option<bool>,
    log_file: Option<String>,
    patterns: Option<Vec<String>>,
    max_entries: Option<usize>,
}

// ============================================================================
// Collectors
// ============================================================================

#[derive(Debug, Default)]
struct BuildTarget {
    name: String,
    description: String,
    container_name: String,
    lunch_target: String,
    aosp_root: String,
    product: String,
    can_emulator: bool,
    can_flash: bool,
}

#[derive(Debug, Default)]
struct ContainerInfo {
    name: String,
    status: String,
    runtime: String,
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: String,
    command: String,
    cwd: String,
}

#[derive(Debug, Default)]
struct Context {
    project_name: String,
    project_type: String,
    targets: Vec<BuildTarget>,
    containers: Vec<ContainerInfo>,
    entry_point: Option<String>,
    available_commands: Vec<String>,
    hints: String,
    command_history: Vec<HistoryEntry>,
}

// ============================================================================
// BuildScript Collector
// ============================================================================

fn collect_build_targets(config: &Config) -> Vec<BuildTarget> {
    let mut targets = Vec::new();

    let scripts_config = match &config.scripts {
        Some(sc) => sc,
        None => return targets,
    };

    let config_dir = match &scripts_config.config_dir {
        Some(dir) => dir.clone(),
        None => return targets,
    };

    let pattern = scripts_config
        .config_pattern
        .as_deref()
        .unwrap_or("*.conf");

    let extract_vars = scripts_config.extract_vars.as_ref();

    let full_pattern = format!("{}/{}", config_dir, pattern);

    if let Ok(entries) = glob::glob(&full_pattern) {
        for entry in entries.flatten() {
            if let Some(target) = parse_config_file(&entry, extract_vars) {
                targets.push(target);
            }
        }
    }

    targets
}

fn parse_config_file(path: &Path, _extract_vars: Option<&Vec<String>>) -> Option<BuildTarget> {
    let content = fs::read_to_string(path).ok()?;

    let mut target = BuildTarget::default();

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        // Parse variable assignments
        if let Some((key, value)) = parse_var_assignment(line) {
            match key.as_str() {
                "TARGET_NAME" => target.name = value,
                "TARGET_DESCRIPTION" => target.description = value,
                "CONTAINER_NAME" => target.container_name = value,
                "LUNCH_TARGET" => target.lunch_target = value,
                "AOSP_ROOT" => target.aosp_root = value,
                "PRODUCT" => target.product = value,
                "CAN_EMULATOR" => target.can_emulator = value == "true",
                "CAN_FLASH" => target.can_flash = value == "true",
                _ => {}
            }
        }
    }

    if target.name.is_empty() {
        // Use filename as fallback
        target.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    Some(target)
}

fn parse_var_assignment(line: &str) -> Option<(String, String)> {
    // Handle: VAR=value or VAR="value"
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].trim().to_string();
    let value = parts[1]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();

    Some((key, value))
}

fn parse_entry_point_commands(entry_point: &str) -> Vec<String> {
    let mut commands = Vec::new();

    if let Ok(content) = fs::read_to_string(entry_point) {
        // Look for command patterns in help text or case statements
        for line in content.lines() {
            let line = line.trim();

            // Parse from help examples: "./aosp.sh build emu"
            if line.contains("./") && line.contains(".sh ") {
                commands.push(line.to_string());
            }
        }
    }

    // Deduplicate and limit
    commands.sort();
    commands.dedup();
    commands.truncate(10);

    commands
}

// ============================================================================
// Container Collector
// ============================================================================

fn collect_containers(config: &Config) -> Vec<ContainerInfo> {
    let mut containers = Vec::new();

    let runtime = config
        .containers
        .as_ref()
        .and_then(|c| c.runtime.as_deref())
        .unwrap_or("podman");

    // Try to get container list
    if let Ok(output) = std::process::Command::new(runtime)
        .args(["ps", "--format", "{{.Names}}\\t{{.Status}}"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 2 {
                    containers.push(ContainerInfo {
                        name: parts[0].to_string(),
                        status: parts[1].to_string(),
                        runtime: runtime.to_string(),
                    });
                }
            }
        }
    }

    containers
}

// ============================================================================
// History Collector
// ============================================================================

fn collect_command_history(config: &Config) -> Vec<HistoryEntry> {
    let history_config = match &config.history {
        Some(hc) if hc.enabled.unwrap_or(true) => hc,
        _ => return Vec::new(),
    };

    let log_file = history_config
        .log_file
        .clone()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{}/.contextkeeper/command-history.jsonl", home)
        });

    let max_entries = history_config.max_entries.unwrap_or(20);

    // Default patterns for AOSP-like environments
    let default_patterns = vec![
        r"lunch\s+\S+".to_string(),
        r"source\s+.*envsetup".to_string(),
        r"export\s+\w+=".to_string(),
        r"m\s+\S+".to_string(),  // AOSP make shortcut
        r"mm\b".to_string(),      // AOSP make module
        r"mma\b".to_string(),     // AOSP make module all
    ];

    let patterns = history_config
        .patterns
        .clone()
        .unwrap_or(default_patterns);

    // Compile regex patterns
    let compiled_patterns: Vec<Regex> = patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    let mut entries = Vec::new();

    // Read log file
    let path = Path::new(&log_file);
    if !path.exists() {
        return entries;
    }

    if let Ok(file) = fs::File::open(path) {
        let reader = io::BufReader::new(file);

        for line in reader.lines().flatten() {
            // Parse JSONL entry
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let command = json["command"].as_str().unwrap_or("");

                // Check if command matches any pattern
                let matches_pattern = compiled_patterns.is_empty() ||
                    compiled_patterns.iter().any(|re| re.is_match(command));

                if matches_pattern && !command.is_empty() {
                    entries.push(HistoryEntry {
                        timestamp: json["timestamp"].as_str().unwrap_or("").to_string(),
                        command: command.to_string(),
                        cwd: json["cwd"].as_str().unwrap_or("").to_string(),
                    });
                }
            }
        }
    }

    // Return last N entries (most recent)
    if entries.len() > max_entries {
        entries.drain(0..entries.len() - max_entries);
    }

    entries
}

// ============================================================================
// Context Aggregator
// ============================================================================

fn collect_context(config: &Config) -> Context {
    let mut ctx = Context::default();

    // Project info
    if let Some(project) = &config.project {
        ctx.project_name = project.name.clone().unwrap_or_default();
        ctx.project_type = project.project_type.clone().unwrap_or_default();
    }

    // Build targets
    ctx.targets = collect_build_targets(config);

    // Containers
    ctx.containers = collect_containers(config);

    // Entry point and commands
    if let Some(scripts) = &config.scripts {
        if let Some(entry) = &scripts.entry_point {
            ctx.entry_point = Some(entry.clone());
            ctx.available_commands = parse_entry_point_commands(entry);
        }
    }

    // Hints
    if let Some(hints) = &config.hints {
        ctx.hints = hints.default.clone().unwrap_or_default();
    }

    // Command history
    ctx.command_history = collect_command_history(config);

    ctx
}

// ============================================================================
// Output Formatters
// ============================================================================

fn format_context_markdown(ctx: &Context) -> String {
    let mut out = String::new();

    out.push_str("# Development Context (ContextKeeper)\n\n");

    // Project info
    if !ctx.project_name.is_empty() {
        out.push_str("## Project\n");
        out.push_str(&format!("- **Name:** {}\n", ctx.project_name));
        if !ctx.project_type.is_empty() {
            out.push_str(&format!("- **Type:** {}\n", ctx.project_type));
        }
        out.push('\n');
    }

    // Hints
    if !ctx.hints.is_empty() {
        out.push_str("## AI Hints (Important)\n");
        out.push_str(&format!("> {}\n\n", ctx.hints));
    }

    // Build targets
    if !ctx.targets.is_empty() {
        out.push_str("## Available Build Targets\n\n");
        out.push_str("| Target | Description | Container | Lunch Target |\n");
        out.push_str("|--------|-------------|-----------|---------------|\n");
        for target in &ctx.targets {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                target.name, target.description, target.container_name, target.lunch_target
            ));
        }
        out.push('\n');

        // Capabilities
        out.push_str("### Target Capabilities\n");
        for target in &ctx.targets {
            let caps: Vec<&str> = [
                if target.can_emulator {
                    Some("emulator")
                } else {
                    None
                },
                if target.can_flash { Some("flash") } else { None },
            ]
            .into_iter()
            .flatten()
            .collect();

            if !caps.is_empty() {
                out.push_str(&format!("- **{}:** {}\n", target.name, caps.join(", ")));
            }
        }
        out.push('\n');
    }

    // Containers
    if !ctx.containers.is_empty() {
        out.push_str("## Active Containers\n");
        for container in &ctx.containers {
            out.push_str(&format!(
                "- **{}** ({}): {}\n",
                container.name, container.runtime, container.status
            ));
        }
        out.push('\n');
    }

    // Available commands
    if !ctx.available_commands.is_empty() {
        out.push_str("## Example Commands\n");
        out.push_str("```bash\n");
        for cmd in &ctx.available_commands {
            out.push_str(&format!("{}\n", cmd));
        }
        out.push_str("```\n");
    }

    // Command history (important for context recovery)
    if !ctx.command_history.is_empty() {
        out.push_str("## Recent Relevant Commands\n");
        out.push_str("These commands were executed in previous sessions (useful after context compression):\n\n");
        out.push_str("| Time | Command |\n");
        out.push_str("|------|--------|\n");
        for entry in &ctx.command_history {
            // Truncate long commands
            let cmd_display = if entry.command.len() > 80 {
                format!("{}...", &entry.command[..77])
            } else {
                entry.command.clone()
            };
            // Escape pipe characters in command
            let cmd_escaped = cmd_display.replace('|', "\\|");
            out.push_str(&format!("| {} | `{}` |\n", entry.timestamp, cmd_escaped));
        }
        out.push('\n');
    }

    out
}

// ============================================================================
// Main
// ============================================================================

fn read_config() -> Config {
    let paths = ["contextkeeper.toml", "context-keeper.toml", ".contextkeeper.toml"];

    for path in paths {
        if Path::new(path).exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(config) = toml::from_str(&content) {
                    return config;
                }
            }
        }
    }

    Config::default()
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let is_context_mode = args.iter().any(|arg| arg == "--context");
    let is_json_mode = args.iter().any(|arg| arg == "--json");

    // Default: output context
    let config = read_config();
    let context = collect_context(&config);

    if is_json_mode {
        // JSON output for programmatic use
        println!("{}", serde_json::to_string_pretty(&context).unwrap_or_default());
    } else {
        // Markdown output (default)
        println!("{}", format_context_markdown(&context));
    }

    Ok(())
}

// Implement Serialize for Context (needed for JSON output)
impl serde::Serialize for Context {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Context", 8)?;
        state.serialize_field("project_name", &self.project_name)?;
        state.serialize_field("project_type", &self.project_type)?;
        state.serialize_field("targets", &self.targets)?;
        state.serialize_field("containers", &self.containers)?;
        state.serialize_field("entry_point", &self.entry_point)?;
        state.serialize_field("available_commands", &self.available_commands)?;
        state.serialize_field("hints", &self.hints)?;
        state.serialize_field("command_history", &self.command_history)?;
        state.end()
    }
}

impl serde::Serialize for BuildTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BuildTarget", 8)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("container_name", &self.container_name)?;
        state.serialize_field("lunch_target", &self.lunch_target)?;
        state.serialize_field("aosp_root", &self.aosp_root)?;
        state.serialize_field("product", &self.product)?;
        state.serialize_field("can_emulator", &self.can_emulator)?;
        state.serialize_field("can_flash", &self.can_flash)?;
        state.end()
    }
}

impl serde::Serialize for ContainerInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ContainerInfo", 3)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("runtime", &self.runtime)?;
        state.end()
    }
}

impl serde::Serialize for HistoryEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("HistoryEntry", 3)?;
        state.serialize_field("timestamp", &self.timestamp)?;
        state.serialize_field("command", &self.command)?;
        state.serialize_field("cwd", &self.cwd)?;
        state.end()
    }
}
