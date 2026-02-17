use regex::Regex;
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, Write};
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
    git: Option<GitConfig>,
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

#[derive(Debug, Deserialize)]
struct GitConfig {
    /// Explicit list of repository paths to check (relative to project root)
    paths: Option<Vec<String>>,
    /// Auto-detect git repositories in subdirectories
    auto_detect: Option<bool>,
    /// Max depth for auto-detection (default: 2)
    scan_depth: Option<usize>,
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
struct GitInfo {
    repo_path: String, // Relative path to the repository
    branch: String,
    is_dirty: bool,
    modified_files: usize,
    untracked_files: usize,
    last_commit_short: String,
}

#[derive(Debug, Clone)]
struct AdbDevice {
    serial: String,
    state: String,
    device_type: String, // "adb" or "fastboot"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TodoItem {
    content: String,
    status: String, // "pending", "in_progress", "completed"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkState {
    saved_at: String,
    trigger: String, // "manual", "pre_compact", "auto"
    task_summary: String,
    working_files: Vec<String>,
    notes: String,
    todos: Vec<TodoItem>,
}

// ============================================================================
// MCP Tool Parameters
// ============================================================================

/// Parameters for get_dev_context tool
#[derive(Debug, Deserialize, JsonSchema)]
struct GetDevContextParams {
    /// Detail level: 'minimal' (~200 tokens), 'normal' (~400 tokens), or 'full' (~1000 tokens). Default: 'normal'
    level: Option<String>,
}

/// Parameters for save_work_state tool
#[derive(Debug, Deserialize, JsonSchema)]
struct SaveWorkStateParams {
    /// Brief summary of current task (required)
    task_summary: String,
    /// List of files currently being worked on (auto-detected from git diff if omitted)
    working_files: Option<Vec<String>>,
    /// Additional notes about current progress
    notes: Option<String>,
    /// Todo items as JSON array: [{"content": "...", "status": "pending|in_progress|completed"}]
    todos: Option<String>,
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
    git_repos: Vec<GitInfo>, // Multiple repositories support
    adb_devices: Vec<AdbDevice>,
    work_state: Option<WorkState>, // Saved work state for recovery
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

    let pattern = scripts_config.config_pattern.as_deref().unwrap_or("*.conf");

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

    let compiled_patterns: Vec<Regex> =
        patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();

    let mut entries = Vec::new();
    let path = Path::new(&log_file);

    if !path.exists() {
        return entries;
    }

    if let Ok(file) = fs::File::open(path) {
        let reader = io::BufReader::new(file);

        for line in reader.lines().map_while(Result::ok) {
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
// Git Collector
// ============================================================================

/// Collect git info from a single repository path
fn collect_git_info_for_path(repo_path: &str) -> Option<GitInfo> {
    let _path = Path::new(repo_path);

    // Check if this path is a git repository
    let is_git = std::process::Command::new("git")
        .args(["-C", repo_path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_git {
        return None;
    }

    let mut info = GitInfo {
        repo_path: repo_path.to_string(),
        ..Default::default()
    };

    // Get current branch
    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", repo_path, "branch", "--show-current"])
        .output()
    {
        if output.status.success() {
            info.branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }

    // If branch is empty, try to get detached HEAD info
    if info.branch.is_empty() {
        if let Ok(output) = std::process::Command::new("git")
            .args(["-C", repo_path, "describe", "--always", "--dirty"])
            .output()
        {
            if output.status.success() {
                info.branch = format!("({})", String::from_utf8_lossy(&output.stdout).trim());
            }
        }
    }

    // Get status (modified and untracked counts)
    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", repo_path, "status", "--porcelain"])
        .output()
    {
        if output.status.success() {
            let status = String::from_utf8_lossy(&output.stdout);
            for line in status.lines() {
                if line.starts_with(" M") || line.starts_with("M ") || line.starts_with("MM") {
                    info.modified_files += 1;
                } else if line.starts_with("??") {
                    info.untracked_files += 1;
                } else if !line.trim().is_empty() {
                    info.modified_files += 1; // Other changes (added, deleted, etc.)
                }
            }
            info.is_dirty = info.modified_files > 0 || info.untracked_files > 0;
        }
    }

    // Get last commit short hash and message
    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", repo_path, "log", "-1", "--format=%h %s"])
        .output()
    {
        if output.status.success() {
            let commit_info = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if commit_info.len() > 50 {
                info.last_commit_short =
                    format!("{}...", &commit_info.chars().take(47).collect::<String>());
            } else {
                info.last_commit_short = commit_info;
            }
        }
    }

    Some(info)
}

/// Auto-detect git repositories in subdirectories
fn find_git_repos(base_path: &str, max_depth: usize) -> Vec<String> {
    let mut repos = Vec::new();
    find_git_repos_recursive(base_path, base_path, 0, max_depth, &mut repos);
    repos.sort();
    repos
}

fn find_git_repos_recursive(
    base_path: &str,
    current_path: &str,
    depth: usize,
    max_depth: usize,
    repos: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }

    let current = Path::new(current_path);

    // Check if current directory is a git repo
    let git_dir = current.join(".git");
    if git_dir.exists() {
        // Use relative path from base
        if let Ok(relative) = current.strip_prefix(base_path) {
            let rel_str = relative.to_string_lossy().to_string();
            if !rel_str.is_empty() {
                repos.push(rel_str);
            }
        }
        return; // Don't recurse into git repos
    }

    // Recurse into subdirectories
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Skip hidden directories and common non-repo directories
                if name.starts_with('.')
                    || name == "node_modules"
                    || name == "target"
                    || name == "out"
                {
                    continue;
                }
                find_git_repos_recursive(
                    base_path,
                    path.to_str().unwrap_or(""),
                    depth + 1,
                    max_depth,
                    repos,
                );
            }
        }
    }
}

/// Collect git info from multiple repositories based on config
fn collect_git_repos(config: &Config) -> Vec<GitInfo> {
    let mut repos = Vec::new();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    // First, check if current directory itself is a git repo
    if let Some(info) = collect_git_info_for_path(&cwd) {
        let mut info = info;
        info.repo_path = ".".to_string();
        repos.push(info);
        return repos; // If root is a git repo, don't scan subdirectories
    }

    // Get paths from config or auto-detect
    let git_config = config.git.as_ref();
    let auto_detect = git_config.and_then(|g| g.auto_detect).unwrap_or(true);
    let explicit_paths = git_config.and_then(|g| g.paths.clone());
    let scan_depth = git_config.and_then(|g| g.scan_depth).unwrap_or(2);

    let paths_to_check: Vec<String> = if let Some(paths) = explicit_paths {
        paths
    } else if auto_detect {
        find_git_repos(&cwd, scan_depth)
    } else {
        Vec::new()
    };

    // Collect info from each path
    for path in paths_to_check {
        let full_path = if Path::new(&path).is_absolute() {
            path.clone()
        } else {
            format!("{}/{}", cwd, path)
        };

        if let Some(mut info) = collect_git_info_for_path(&full_path) {
            info.repo_path = path;
            repos.push(info);
        }
    }

    // Sort by path for consistent output
    repos.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));

    // Limit to reasonable number
    repos.truncate(10);

    repos
}

// ============================================================================
// ADB/Fastboot Collector
// ============================================================================

fn collect_adb_devices() -> Vec<AdbDevice> {
    let mut devices = Vec::new();

    // Collect ADB devices
    if let Ok(output) = std::process::Command::new("adb")
        .args(["devices", "-l"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                // Skip "List of devices attached"
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let serial = parts[0].to_string();
                    let state = parts[1].to_string();

                    // Skip offline devices
                    if state == "offline" {
                        continue;
                    }

                    devices.push(AdbDevice {
                        serial,
                        state,
                        device_type: "adb".to_string(),
                    });
                }
            }
        }
    }

    // Collect Fastboot devices
    if let Ok(output) = std::process::Command::new("fastboot")
        .args(["devices", "-l"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = line.split_whitespace().collect();
                if !parts.is_empty() {
                    let serial = parts[0].to_string();
                    devices.push(AdbDevice {
                        serial,
                        state: "fastboot".to_string(),
                        device_type: "fastboot".to_string(),
                    });
                }
            }
        }
    }

    devices
}

// ============================================================================
// Work State Management
// ============================================================================

fn get_work_state_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{}/.contextkeeper/work-state.json", home)
}

fn ensure_contextkeeper_dir() -> io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{}/.contextkeeper", home);
    fs::create_dir_all(&dir)?;
    Ok(())
}

fn save_work_state_to_file(state: &WorkState) -> io::Result<()> {
    ensure_contextkeeper_dir()?;
    let path = get_work_state_path();
    let json = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    let mut file = fs::File::create(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

fn load_work_state_from_file() -> Option<WorkState> {
    let path = get_work_state_path();
    if !Path::new(&path).exists() {
        return None;
    }

    fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

/// Collect working files from git diff (for PreCompact hook)
fn collect_working_files() -> Vec<String> {
    let mut files = Vec::new();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    // Try to get modified files from all git repos
    if let Ok(output) = std::process::Command::new("bash")
        .args(["-c", &format!(
            "cd '{}' && find . -maxdepth 3 -name '.git' -type d 2>/dev/null | while read gitdir; do \
             repo=$(dirname \"$gitdir\"); \
             git -C \"$repo\" diff --name-only 2>/dev/null | sed \"s|^|$repo/|\" ; \
             done | head -20",
            cwd
        )])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    // Clean up path (remove leading ./)
                    let clean_path = line.trim_start_matches("./");
                    files.push(clean_path.to_string());
                }
            }
        }
    }

    files
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
    ctx.git_repos = collect_git_repos(config);
    ctx.adb_devices = collect_adb_devices();
    ctx.work_state = load_work_state_from_file();
    ctx
}

// ============================================================================
// Output Formatter (Hierarchical: minimal / normal / full)
// ============================================================================

/// Helper: format git status string
fn format_git_status(git: &GitInfo) -> String {
    if git.is_dirty {
        if git.modified_files > 0 && git.untracked_files > 0 {
            format!("{}M {}U", git.modified_files, git.untracked_files)
        } else if git.modified_files > 0 {
            format!("{}M", git.modified_files)
        } else {
            format!("{}U", git.untracked_files)
        }
    } else {
        "clean".to_string()
    }
}

/// Helper: format work state section
fn format_work_state(work_state: &WorkState) -> String {
    let mut out = String::new();
    out.push_str("## Saved Work State\n");
    out.push_str(&format!("- **Saved at:** {}\n", work_state.saved_at));

    if !work_state.task_summary.is_empty() {
        out.push_str(&format!("- **Task:** {}\n", work_state.task_summary));
    }

    if !work_state.working_files.is_empty() {
        out.push_str("- **Working files:**\n");
        for file in &work_state.working_files {
            out.push_str(&format!("  - {}\n", file));
        }
    }

    if !work_state.notes.is_empty() {
        out.push_str(&format!("- **Notes:** {}\n", work_state.notes));
    }

    if !work_state.todos.is_empty() {
        out.push_str("- **Todos:**\n");
        for todo in &work_state.todos {
            let checkbox = match todo.status.as_str() {
                "completed" => "[x]",
                "in_progress" => "[>]",
                _ => "[ ]",
            };
            out.push_str(&format!("  - {} {}\n", checkbox, todo.content));
        }
    }

    out.push('\n');
    out
}

/// Minimal format (~200 tokens) - for recovery after compression
fn format_minimal(ctx: &Context) -> String {
    let mut out = String::new();

    out.push_str("# Context Recovery (Minimal)\n\n");

    // Work state is most important for recovery
    if let Some(ws) = &ctx.work_state {
        if !ws.task_summary.is_empty() {
            out.push_str(&format!("**Task:** {}\n", ws.task_summary));
        }
        if !ws.working_files.is_empty() {
            let files: Vec<&str> = ws.working_files.iter().map(|s| s.as_str()).collect();
            out.push_str(&format!("**Files:** {}\n", files.join(", ")));
        }
        if !ws.notes.is_empty() {
            out.push_str(&format!("**Notes:** {}\n", ws.notes));
        }
        out.push('\n');
    }

    // Show only dirty repos
    let dirty_repos: Vec<&GitInfo> = ctx.git_repos.iter().filter(|r| r.is_dirty).collect();
    if !dirty_repos.is_empty() {
        out.push_str("**Changed repos:** ");
        let repo_strs: Vec<String> = dirty_repos
            .iter()
            .map(|r| format!("{} ({})", r.repo_path, format_git_status(r)))
            .collect();
        out.push_str(&repo_strs.join(", "));
        out.push('\n');
    }

    // Device (one line)
    if !ctx.adb_devices.is_empty() {
        let device = &ctx.adb_devices[0];
        out.push_str(&format!(
            "**Device:** {} ({})\n",
            device.serial, device.device_type
        ));
    }

    out.push_str("\n---\n");
    out.push_str("*Run `get_dev_context` with level=\"normal\" or \"full\" for more details.*\n");

    out
}

/// Normal format (~400 tokens) - balanced info
fn format_normal(ctx: &Context) -> String {
    let mut out = String::new();

    out.push_str("# Development Context\n\n");

    // Work state
    if let Some(ws) = &ctx.work_state {
        out.push_str(&format_work_state(ws));
    }

    // AI Hints
    if !ctx.hints.is_empty() {
        out.push_str("## AI Hints\n");
        out.push_str(&format!("> {}\n\n", ctx.hints));
    }

    // Git Status (dirty repos only)
    let dirty_repos: Vec<&GitInfo> = ctx.git_repos.iter().filter(|r| r.is_dirty).collect();
    if !dirty_repos.is_empty() {
        out.push_str("## Git Status (changes only)\n\n");
        out.push_str("| Repository | Branch | Status |\n");
        out.push_str("|------------|--------|--------|\n");
        for git in dirty_repos {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                git.repo_path,
                git.branch,
                format_git_status(git)
            ));
        }
        out.push('\n');
    }

    // Active containers
    if !ctx.containers.is_empty() {
        out.push_str("## Active Containers\n");
        for container in &ctx.containers {
            out.push_str(&format!("- {} ({})\n", container.name, container.status));
        }
        out.push('\n');
    }

    // Connected devices
    if !ctx.adb_devices.is_empty() {
        out.push_str("## Connected Devices\n");
        for device in &ctx.adb_devices {
            out.push_str(&format!(
                "- {} ({}, {})\n",
                device.serial, device.state, device.device_type
            ));
        }
        out.push('\n');
    }

    out.push_str("---\n");
    out.push_str("*Run `get_dev_context` with level=\"full\" for complete information.*\n");

    out
}

/// Full format (~1000 tokens) - complete information
fn format_full(ctx: &Context) -> String {
    let mut out = String::new();

    out.push_str("# Development Context (Full)\n\n");

    // Project info
    if !ctx.project_name.is_empty() {
        out.push_str("## Project\n");
        out.push_str(&format!("- **Name:** {}\n", ctx.project_name));
        if !ctx.project_type.is_empty() {
            out.push_str(&format!("- **Type:** {}\n", ctx.project_type));
        }
        out.push('\n');
    }

    // Work state
    if let Some(ws) = &ctx.work_state {
        out.push_str(&format_work_state(ws));
    }

    // AI Hints
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

        out.push_str("### Target Capabilities\n");
        for target in &ctx.targets {
            let caps: Vec<&str> = [
                if target.can_emulator {
                    Some("emulator")
                } else {
                    None
                },
                if target.can_flash {
                    Some("flash")
                } else {
                    None
                },
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

    // Example commands
    if !ctx.available_commands.is_empty() {
        out.push_str("## Example Commands\n");
        out.push_str("```bash\n");
        for cmd in &ctx.available_commands {
            out.push_str(&format!("{}\n", cmd));
        }
        out.push_str("```\n");
    }

    // Command history
    if !ctx.command_history.is_empty() {
        out.push_str("## Recent Relevant Commands\n");
        out.push_str(
            "These commands were executed in previous sessions (useful after context compression):\n\n",
        );
        out.push_str("| Time | Command |\n");
        out.push_str("|------|--------|\n");
        for entry in &ctx.command_history {
            let cmd_display = if entry.command.chars().count() > 80 {
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

    // Git information (ALL repositories)
    if !ctx.git_repos.is_empty() {
        out.push_str("## Git Status\n\n");
        out.push_str("| Repository | Branch | Status | Last Commit |\n");
        out.push_str("|------------|--------|--------|-------------|\n");

        for git in &ctx.git_repos {
            let commit = git.last_commit_short.replace('|', "\\|");
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                git.repo_path,
                git.branch,
                format_git_status(git),
                commit
            ));
        }
        out.push('\n');
    }

    // ADB/Fastboot devices
    if !ctx.adb_devices.is_empty() {
        out.push_str("## Connected Devices\n");
        out.push_str("| Serial | State | Type |\n");
        out.push_str("|--------|-------|------|\n");
        for device in &ctx.adb_devices {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                device.serial, device.state, device.device_type
            ));
        }
        out.push('\n');
    }

    out
}

/// Main formatter dispatcher
fn format_context_markdown(ctx: &Context, level: &str) -> String {
    match level {
        "minimal" => format_minimal(ctx),
        "normal" => format_normal(ctx),
        "full" => format_full(ctx),
        _ => format_normal(ctx), // Default to normal
    }
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

impl Default for ContextKeeperService {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl ContextKeeperService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Get development context. Use level='minimal' after compression (~200 tokens), 'normal' for balanced info (~400 tokens), or 'full' for complete details (~1000 tokens). Default is 'normal'."
    )]
    async fn get_dev_context(
        &self,
        params: Parameters<GetDevContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let config = read_config();
        let context = collect_context(&config);
        let level_str = params.0.level.as_deref().unwrap_or("normal");
        let markdown = format_context_markdown(&context, level_str);

        Ok(CallToolResult::success(vec![Content::text(markdown)]))
    }

    #[tool(
        description = "Save current work state for recovery after context compression. Call this before compression or at task milestones."
    )]
    async fn save_work_state(
        &self,
        params: Parameters<SaveWorkStateParams>,
    ) -> Result<CallToolResult, McpError> {
        let SaveWorkStateParams {
            task_summary,
            working_files,
            notes,
            todos,
        } = params.0;

        // Parse todos if provided
        let todo_items: Vec<TodoItem> = todos
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();

        // Auto-collect working files if not provided
        let files = working_files.unwrap_or_else(collect_working_files);

        let state = WorkState {
            saved_at: chrono::Utc::now().to_rfc3339(),
            trigger: "manual".to_string(),
            task_summary,
            working_files: files,
            notes: notes.unwrap_or_default(),
            todos: todo_items,
        };

        match save_work_state_to_file(&state) {
            Ok(_) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Work state saved successfully.\n\n\
                - Task: {}\n\
                - Files: {}\n\
                - Todos: {} items\n\n\
                This state will be included in `get_dev_context` output after compression.",
                state.task_summary,
                state.working_files.len(),
                state.todos.len()
            ))])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Failed to save work state: {}",
                e
            ))])),
        }
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
// Init Wizard
// ============================================================================

/// Detect project type based on directory contents
fn detect_project_type() -> Option<&'static str> {
    // Check for AOSP
    if Path::new("build/envsetup.sh").exists() || Path::new("build/make/envsetup.sh").exists() {
        return Some("aosp");
    }

    // Check for ROS/ROS2
    if Path::new("package.xml").exists() {
        return Some("ros");
    }
    if Path::new("src").is_dir() {
        // Check for colcon/catkin workspace
        if let Ok(entries) = fs::read_dir("src") {
            for entry in entries.flatten() {
                let pkg_xml = entry.path().join("package.xml");
                if pkg_xml.exists() {
                    return Some("ros");
                }
            }
        }
    }

    // Check for Yocto
    if Path::new("meta").is_dir() || Path::new("poky").is_dir() {
        return Some("yocto");
    }
    if let Ok(entries) = fs::read_dir(".") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("meta-") {
                return Some("yocto");
            }
        }
    }

    None
}

/// Detect available container runtime
fn detect_container_runtime() -> Option<&'static str> {
    // Check podman first (preferred for rootless)
    if std::process::Command::new("podman")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some("podman");
    }

    // Check docker
    if std::process::Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some("docker");
    }

    None
}

/// Get current directory name as default project name
fn get_default_project_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "my-project".to_string())
}

/// Prompt user for input with default value
fn prompt(question: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{}: ", question);
    } else {
        print!("{} [{}]: ", question, default);
    }
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();

    if input.is_empty() {
        default.to_string()
    } else {
        input.to_string()
    }
}

/// Prompt for yes/no with default
fn prompt_yes_no(question: &str, default: bool) -> bool {
    let default_str = if default { "Y/n" } else { "y/N" };
    print!("{} [{}]: ", question, default_str);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();

    match input.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    }
}

/// Generate default history patterns based on project type
fn get_default_history_patterns(project_type: &str) -> Vec<&'static str> {
    match project_type {
        "aosp" => vec![
            r"lunch\s+\S+",
            r"source.*envsetup",
            r"export\s+\w+=",
            r"m\s+\S+",
            r"mm\b",
            r"mma\b",
        ],
        "ros" => vec![
            r"source.*setup\.bash",
            r"colcon\s+build",
            r"catkin_make",
            r"ros2\s+run",
            r"ros2\s+launch",
            r"roslaunch",
        ],
        "yocto" => vec![
            r"source.*oe-init",
            r"bitbake\s+\S+",
            r"MACHINE=",
            r"devtool\s+\S+",
        ],
        _ => vec![r"export\s+\w+=", r"source\s+"],
    }
}

/// Run the init wizard
fn run_init_wizard() -> io::Result<()> {
    println!("\n🔧 ContextKeeper Setup Wizard\n");

    // Check if config already exists
    if Path::new("contextkeeper.toml").exists()
        && !prompt_yes_no("contextkeeper.toml already exists. Overwrite?", false)
    {
        println!("Aborted.");
        return Ok(());
    }

    // Project name
    let default_name = get_default_project_name();
    let project_name = prompt("Project name", &default_name);

    // Project type
    let detected_type = detect_project_type();
    let type_hint = detected_type
        .map(|t| format!("detected: {}", t))
        .unwrap_or_else(|| "aosp/ros/yocto/custom".to_string());
    let project_type = prompt(
        &format!("Project type ({})", type_hint),
        detected_type.unwrap_or("custom"),
    );

    // Container runtime
    let detected_runtime = detect_container_runtime();
    let runtime_hint = detected_runtime
        .map(|r| format!("detected: {}", r))
        .unwrap_or_else(|| "podman/docker/none".to_string());
    let container_runtime = prompt(
        &format!("Container runtime ({})", runtime_hint),
        detected_runtime.unwrap_or("none"),
    );

    // Build scripts (optional)
    let entry_point = prompt("Build script entry point (optional)", "");
    let config_dir = if !entry_point.is_empty() {
        prompt("Config directory (optional)", "")
    } else {
        String::new()
    };

    // AI hints
    let default_hint = if container_runtime != "none" {
        "Build commands must be executed inside the container."
    } else {
        ""
    };
    let ai_hint = prompt("AI hint for this project", default_hint);

    // Generate TOML
    let mut toml_content = String::new();

    toml_content.push_str("# ContextKeeper Configuration\n");
    toml_content.push_str("# https://github.com/sat0sh-dev/context-keeper\n\n");

    toml_content.push_str("[project]\n");
    toml_content.push_str(&format!("name = \"{}\"\n", project_name));
    toml_content.push_str(&format!("type = \"{}\"\n", project_type));
    toml_content.push('\n');

    if !entry_point.is_empty() {
        toml_content.push_str("[scripts]\n");
        toml_content.push_str(&format!("entry_point = \"{}\"\n", entry_point));
        if !config_dir.is_empty() {
            toml_content.push_str(&format!("config_dir = \"{}\"\n", config_dir));
            toml_content.push_str("config_pattern = \"*.conf\"\n");
        }
        toml_content.push('\n');
    }

    if container_runtime != "none" {
        toml_content.push_str("[containers]\n");
        toml_content.push_str(&format!("runtime = \"{}\"\n", container_runtime));
        toml_content.push('\n');
    }

    if !ai_hint.is_empty() {
        toml_content.push_str("[hints]\n");
        toml_content.push_str(&format!("default = \"{}\"\n", ai_hint));
        toml_content.push('\n');
    }

    // History config with type-appropriate patterns
    toml_content.push_str("[history]\n");
    toml_content.push_str("enabled = true\n");
    toml_content.push_str("patterns = [\n");
    for pattern in get_default_history_patterns(&project_type) {
        // Escape backslashes for TOML
        let escaped = pattern.replace('\\', "\\\\");
        toml_content.push_str(&format!("    \"{}\",\n", escaped));
    }
    toml_content.push_str("]\n");
    toml_content.push_str("max_entries = 20\n");
    toml_content.push('\n');

    // Git config
    toml_content.push_str("[git]\n");
    toml_content.push_str("auto_detect = true\n");
    toml_content.push_str("scan_depth = 2\n");

    // Write file
    fs::write("contextkeeper.toml", &toml_content)?;

    println!("\n✅ Created contextkeeper.toml");
    println!("\nNext steps:");
    println!("  1. Review and customize contextkeeper.toml");
    println!("  2. Test with: context-keeper --context");
    println!("  3. Add to Claude Code: ./install.sh (or see README)");

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Init wizard mode
    // Usage: context-keeper init
    if args.iter().any(|arg| arg == "init") {
        run_init_wizard()?;
        return Ok(());
    }

    // CLI mode: output context directly
    // Usage: context-keeper --context [minimal|normal|full]
    if args.iter().any(|arg| arg == "--context" || arg == "-c") {
        let config = read_config();
        let context = collect_context(&config);

        // Check for level argument
        let level = args
            .iter()
            .position(|arg| arg == "--context" || arg == "-c")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("normal");

        println!("{}", format_context_markdown(&context, level));
        return Ok(());
    }

    // Save state mode (for PreCompact hook)
    // Usage: context-keeper --save-state "task description"
    if let Some(pos) = args.iter().position(|arg| arg == "--save-state") {
        let task_summary = args.get(pos + 1).cloned().unwrap_or_default();
        let files = collect_working_files();

        let state = WorkState {
            saved_at: chrono::Utc::now().to_rfc3339(),
            trigger: "pre_compact".to_string(),
            task_summary,
            working_files: files,
            notes: String::new(),
            todos: Vec::new(),
        };

        match save_work_state_to_file(&state) {
            Ok(_) => println!(
                "Work state saved: {} files tracked",
                state.working_files.len()
            ),
            Err(e) => eprintln!("Failed to save work state: {}", e),
        }
        return Ok(());
    }

    // MCP Server mode (default)
    let service = ContextKeeperService::new();
    let server = service.serve(stdio()).await?;
    server.waiting().await?;

    Ok(())
}
