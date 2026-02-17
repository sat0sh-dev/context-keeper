use regex::Regex;
use rmcp::{
    handler::server::tool::ToolRouter,
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

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
    #[allow(dead_code)]
    extract_vars: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ContainersConfig {
    runtime: Option<String>,
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
// Collector Data Structures
// ============================================================================

#[derive(Debug, Default, Clone)]
struct BuildTarget {
    name: String,
    description: String,
    container_name: String,
    lunch_target: String,
    can_emulator: bool,
    can_flash: bool,
}

#[derive(Debug, Default, Clone)]
struct ContainerInfo {
    name: String,
    status: String,
    runtime: String,
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: String,
    command: String,
}

#[derive(Debug, Default, Clone)]
struct Context {
    project_name: String,
    project_type: String,
    targets: Vec<BuildTarget>,
    containers: Vec<ContainerInfo>,
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

    let full_pattern = format!("{}/{}", config_dir, pattern);

    if let Ok(entries) = glob::glob(&full_pattern) {
        for entry in entries.flatten() {
            if let Some(target) = parse_config_file(&entry) {
                targets.push(target);
            }
        }
    }

    targets
}

fn parse_config_file(path: &Path) -> Option<BuildTarget> {
    let content = fs::read_to_string(path).ok()?;
    let mut target = BuildTarget::default();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if let Some((key, value)) = parse_var_assignment(line) {
            match key.as_str() {
                "TARGET_NAME" => target.name = value,
                "TARGET_DESCRIPTION" => target.description = value,
                "CONTAINER_NAME" => target.container_name = value,
                "LUNCH_TARGET" => target.lunch_target = value,
                "CAN_EMULATOR" => target.can_emulator = value == "true",
                "CAN_FLASH" => target.can_flash = value == "true",
                _ => {}
            }
        }
    }

    if target.name.is_empty() {
        target.name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    Some(target)
}

fn parse_var_assignment(line: &str) -> Option<(String, String)> {
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
        for line in content.lines() {
            let line = line.trim();
            if line.contains("./") && line.contains(".sh ") {
                commands.push(line.to_string());
            }
        }
    }

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

    let log_file = history_config.log_file.clone().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/.contextkeeper/command-history.jsonl", home)
    });

    let max_entries = history_config.max_entries.unwrap_or(20);

    let default_patterns = vec![
        r"lunch\s+\S+".to_string(),
        r"source\s+.*envsetup".to_string(),
        r"export\s+\w+=".to_string(),
        r"m\s+\S+".to_string(),
        r"mm\b".to_string(),
        r"mma\b".to_string(),
    ];

    let patterns = history_config.patterns.clone().unwrap_or(default_patterns);

    let compiled_patterns: Vec<Regex> = patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    let mut entries = Vec::new();
    let path = Path::new(&log_file);

    if !path.exists() {
        return entries;
    }

    if let Ok(file) = fs::File::open(path) {
        let reader = io::BufReader::new(file);

        for line in reader.lines().flatten() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let command = json["command"].as_str().unwrap_or("");
                let matches_pattern = compiled_patterns.is_empty()
                    || compiled_patterns.iter().any(|re| re.is_match(command));

                if matches_pattern && !command.is_empty() {
                    entries.push(HistoryEntry {
                        timestamp: json["timestamp"].as_str().unwrap_or("").to_string(),
                        command: command.to_string(),
                    });
                }
            }
        }
    }

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

    if let Some(project) = &config.project {
        ctx.project_name = project.name.clone().unwrap_or_default();
        ctx.project_type = project.project_type.clone().unwrap_or_default();
    }

    ctx.targets = collect_build_targets(config);
    ctx.containers = collect_containers(config);

    if let Some(scripts) = &config.scripts {
        if let Some(entry) = &scripts.entry_point {
            ctx.available_commands = parse_entry_point_commands(entry);
        }
    }

    if let Some(hints) = &config.hints {
        ctx.hints = hints.default.clone().unwrap_or_default();
    }

    ctx.command_history = collect_command_history(config);
    ctx
}

// ============================================================================
// Output Formatter
// ============================================================================

fn format_context_markdown(ctx: &Context) -> String {
    let mut out = String::new();

    out.push_str("# Development Context (ContextKeeper)\n\n");

    if !ctx.project_name.is_empty() {
        out.push_str("## Project\n");
        out.push_str(&format!("- **Name:** {}\n", ctx.project_name));
        if !ctx.project_type.is_empty() {
            out.push_str(&format!("- **Type:** {}\n", ctx.project_type));
        }
        out.push('\n');
    }

    if !ctx.hints.is_empty() {
        out.push_str("## AI Hints (Important)\n");
        out.push_str(&format!("> {}\n\n", ctx.hints));
    }

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

    if !ctx.available_commands.is_empty() {
        out.push_str("## Example Commands\n");
        out.push_str("```bash\n");
        for cmd in &ctx.available_commands {
            out.push_str(&format!("{}\n", cmd));
        }
        out.push_str("```\n");
    }

    if !ctx.command_history.is_empty() {
        out.push_str("## Recent Relevant Commands\n");
        out.push_str(
            "These commands were executed in previous sessions (useful after context compression):\n\n",
        );
        out.push_str("| Time | Command |\n");
        out.push_str("|------|--------|\n");
        for entry in &ctx.command_history {
            let cmd_display = if entry.command.chars().count() > 80 {
                // Safely truncate at character boundary
                let truncated: String = entry.command.chars().take(77).collect();
                format!("{}...", truncated)
            } else {
                entry.command.clone()
            };
            let cmd_escaped = cmd_display.replace('|', "\\|");
            out.push_str(&format!("| {} | `{}` |\n", entry.timestamp, cmd_escaped));
        }
        out.push('\n');
    }

    out
}

// ============================================================================
// Config Reader
// ============================================================================

fn read_config() -> Config {
    let paths = [
        "contextkeeper.toml",
        "context-keeper.toml",
        ".contextkeeper.toml",
    ];

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

// ============================================================================
// MCP Server Implementation
// ============================================================================

#[derive(Clone)]
pub struct ContextKeeperService {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ContextKeeperService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Get current development environment context including build targets, containers, command history, and configuration. Call this when context is unclear or after context compression.")]
    async fn get_dev_context(&self) -> Result<CallToolResult, McpError> {
        let config = read_config();
        let context = collect_context(&config);
        let markdown = format_context_markdown(&context);

        Ok(CallToolResult::success(vec![Content::text(markdown)]))
    }
}

#[tool_handler]
impl ServerHandler for ContextKeeperService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "context-keeper".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "ContextKeeper provides development environment context. \
                 Call get_dev_context to retrieve build targets, containers, \
                 and recent commands."
                    .into(),
            ),
        }
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // CLI mode: output context directly
    if args.iter().any(|arg| arg == "--context" || arg == "-c") {
        let config = read_config();
        let context = collect_context(&config);
        println!("{}", format_context_markdown(&context));
        return Ok(());
    }

    // MCP Server mode (default)
    let service = ContextKeeperService::new();
    let server = service.serve(stdio()).await?;
    server.waiting().await?;

    Ok(())
}
