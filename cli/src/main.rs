// driver — minimal track planner.
//
// Operates on files under driver/ in the current project. Discovers the
// project root by walking up from the cwd looking for driver/plan.md
// (or driver/principles.md). Driver is one evolving plan.md — no
// per-track folders, no registry file.
//
// See DESIGN.md in the driver repo for the data model.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Minimal track planner.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Full overview: plan, claim, recent ticks, open questions.
    Status,
    /// Print the next runnable task.
    Next {
        #[arg(long)]
        json: bool,
    },
    /// List all tasks with their status.
    Tasks,
    /// Tick a task as done.
    Tick { slug: String },
    /// Untick a task (mark open again).
    Untick { slug: String },
    /// Rename a task: updates plan.md and renames <slug>_* files.
    Rename {
        old_slug: String,
        new_slug: String,
    },
    /// Start a claim on a task. Configures the Stop hook to keep the
    /// agent working until the task is ticked, halted, or budget is hit.
    /// `--max-turns` defaults to `ceil(estimate * 1.3 / 5) * 5` when the
    /// task has an estimate, otherwise 50.
    Claim {
        slug: String,
        #[arg(long)]
        max_turns: Option<u32>,
    },
    /// End the current claim (idempotent).
    Release,
    /// Stop-hook callback. Exits 2 if the active claim is incomplete.
    Gate,
    /// Verify the local Driver setup (binary on PATH, hook installed,
    /// project layout). If the Stop hook is missing, prints the
    /// settings.json snippet to paste.
    Doctor,
    /// Show estimate-vs-actual statistics from driver/.history.jsonl.
    Stats,
    /// Append an open question to <slug>_questions.md. By default
    /// non-blocking — the agent can keep working on parts that don't
    /// depend on the answer. Pass `--halt` for "fully stuck": that
    /// marks the task as halted, releases the active claim if any, and
    /// makes dependents transitively unreachable until the question is
    /// answered.
    Ask {
        slug: String,
        /// Question text (one sentence).
        question: String,
        /// Tag the question with a principles.md rule name. Required
        /// if the task's diff trips a mechanical trigger.
        #[arg(long)]
        rule: Option<String>,
        /// Optional context paragraph.
        #[arg(long)]
        context: Option<String>,
        /// Mark the question as a hard halt: task can't make further
        /// progress until answered.
        #[arg(long)]
        halt: bool,
    },
    /// List open questions across the project.
    Questions,
    /// Record an answer to a logged question. Replaces `_pending_` on
    /// the matching Q's answer line.
    Answer {
        slug: String,
        /// 1-based question index, as shown by `driver questions`.
        index: u32,
        answer: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // gate has special exit semantics: 2 means "block the stop".
    if let Cmd::Gate = cli.command {
        return cmd_gate();
    }
    let result = match cli.command {
        Cmd::Status => cmd_status(),
        Cmd::Next { json } => cmd_next(json),
        Cmd::Tasks => cmd_tasks(),
        Cmd::Tick { slug } => cmd_tick(&slug, true),
        Cmd::Untick { slug } => cmd_tick(&slug, false),
        Cmd::Rename { old_slug, new_slug } => cmd_rename(&old_slug, &new_slug),
        Cmd::Claim { slug, max_turns } => cmd_claim(&slug, max_turns),
        // (cmd_claim resolves max_turns from the plan estimate when None.)
        Cmd::Release => cmd_release(),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Stats => cmd_stats(),
        Cmd::Ask {
            slug,
            question,
            rule,
            context,
            halt,
        } => cmd_ask(&slug, &question, rule.as_deref(), context.as_deref(), halt),
        Cmd::Questions => cmd_questions(),
        Cmd::Answer {
            slug,
            index,
            answer,
        } => cmd_answer(&slug, index, &answer),
        Cmd::Gate => unreachable!(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("driver: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---- discovery ----

fn find_driver_root() -> Result<PathBuf, String> {
    let mut dir = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    loop {
        let candidate = dir.join("driver");
        if candidate.join("plan.md").exists() || candidate.join("principles.md").exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err(
                "no driver/ project found in this directory or any parent (expected driver/plan.md or driver/principles.md)".into(),
            );
        }
    }
}

// ---- plan.md ----

#[derive(Debug, Clone)]
struct Task {
    done: bool,
    slug: String,
    estimate: Option<u32>,
    depends: Vec<String>,
    /// Line index of the task's first line in the source.
    line: usize,
    description: String,
}

/// Read `driver/plan.md`. Returns the resolved path, the raw lines
/// (for write-back), and the parsed task list.
fn read_plan(root: &Path) -> Result<(PathBuf, Vec<String>, Vec<Task>), String> {
    let plan_md = root.join("plan.md");
    if !plan_md.exists() {
        return Err(format!(
            "no plan.md at {}",
            plan_md.display()
        ));
    }
    let text = fs::read_to_string(&plan_md).map_err(|e| format!("read plan.md: {e}"))?;
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let tasks = parse_tasks(&lines)?;
    Ok((plan_md, lines, tasks))
}

/// Convenience: just the project root.
fn project_root() -> Result<PathBuf, String> {
    find_driver_root()
}

fn parse_tasks(lines: &[String]) -> Result<Vec<Task>, String> {
    let mut tasks: Vec<Task> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let done = t.starts_with("- [x]");
        if !done && !t.starts_with("- [ ]") {
            // Continuation of previous task's description?
            if let Some(last) = tasks.last_mut() {
                if !line.is_empty() && line.starts_with("  ") {
                    if !last.description.is_empty() {
                        last.description.push(' ');
                    }
                    last.description.push_str(line.trim());
                }
            }
            continue;
        }
        // Parse "- [x] **slug** (~K turns) [depends: a, b]"
        let body = t[5..].trim_start();
        let (slug, after_slug) = parse_bold_slug(body)
            .ok_or_else(|| format!("line {}: expected **slug** after checkbox", i + 1))?;
        let estimate = parse_estimate(&after_slug);
        let depends = parse_depends(&after_slug);
        tasks.push(Task {
            done,
            slug,
            estimate,
            depends,
            line: i,
            description: String::new(),
        });
    }
    // Sanity: dependency targets must exist.
    let slugs: HashSet<&str> = tasks.iter().map(|t| t.slug.as_str()).collect();
    for t in &tasks {
        for dep in &t.depends {
            if !slugs.contains(dep.as_str()) {
                return Err(format!(
                    "task '{}' depends on unknown task '{dep}'",
                    t.slug
                ));
            }
        }
    }
    Ok(tasks)
}

fn parse_bold_slug(s: &str) -> Option<(String, String)> {
    let rest = s.strip_prefix("**")?;
    let end = rest.find("**")?;
    Some((rest[..end].to_string(), rest[end + 2..].to_string()))
}

fn parse_estimate(s: &str) -> Option<u32> {
    // first "(~K turns)" or "(~K t)" -> K
    let i = s.find('(')?;
    let j = s[i..].find(')')?;
    let inner = &s[i + 1..i + j];
    let inner = inner.trim_start_matches('~').trim();
    inner.split_whitespace().next()?.parse().ok()
}

fn parse_depends(s: &str) -> Vec<String> {
    // "[depends: a, b, c]"
    let Some(i) = s.find("[depends:") else {
        return Vec::new();
    };
    let s = &s[i + "[depends:".len()..];
    let Some(j) = s.find(']') else {
        return Vec::new();
    };
    s[..j]
        .split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

// ---- DAG semantics ----

/// A task is directly blocked iff its questions file has at least one
/// unanswered halt-marked question.
fn directly_blocked(root: &Path, slug: &str) -> bool {
    read_questions(root, slug)
        .iter()
        .any(|q| q.halt && !q.answered)
}

fn transitive_blockers(root: &Path, tasks: &[Task]) -> HashMap<String, Vec<String>> {
    // Returns slug -> list of upstream blocked slugs (direct or transitive).
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    for t in tasks {
        let mut blockers: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();
        queue.push_back(t.slug.as_str());
        while let Some(s) = queue.pop_front() {
            if !seen.insert(s.to_string()) {
                continue;
            }
            if s != t.slug && directly_blocked(root, s) {
                blockers.push(s.to_string());
            }
            if let Some(task) = by_slug.get(s) {
                for d in &task.depends {
                    queue.push_back(d.as_str());
                }
            }
        }
        // Also: a task is "blocked" if it's directly blocked itself.
        if directly_blocked(root, &t.slug) {
            blockers.insert(0, t.slug.clone());
        }
        result.insert(t.slug.clone(), blockers);
    }
    result
}

fn is_runnable(t: &Task, tasks: &[Task], blockers: &HashMap<String, Vec<String>>) -> bool {
    if t.done {
        return false;
    }
    if blockers.get(&t.slug).is_some_and(|b| !b.is_empty()) {
        return false;
    }
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|x| (x.slug.as_str(), x)).collect();
    for dep in &t.depends {
        if let Some(dep_task) = by_slug.get(dep.as_str()) {
            if !dep_task.done {
                return false;
            }
        }
    }
    true
}

// ---- commands ----

/// Full overview: plan summary, active claim, recent ticks, open
/// questions. Sufficient both as a daily check and as session re-entry
/// after Claude Code context compaction.
fn cmd_status() -> Result<(), String> {
    let root = project_root()?;
    println!("Driver status — {}", root.display());
    let plan = read_plan(&root);

    // == plan ==
    println!("\n== plan ==");
    match &plan {
        Ok((_, _, tasks)) => {
            let total = tasks.len();
            let complete = tasks.iter().filter(|t| t.done).count();
            let blockers = transitive_blockers(&root, tasks);
            let runnable: Vec<&Task> = tasks
                .iter()
                .filter(|t| is_runnable(t, tasks, &blockers))
                .collect();
            let blocked_count = tasks
                .iter()
                .filter(|t| !t.done && blockers.get(&t.slug).is_some_and(|b| !b.is_empty()))
                .count();
            println!(
                "  {complete}/{total} done, {} runnable, {blocked_count} blocked",
                runnable.len()
            );
            if let Some(t) = runnable.first() {
                let est = t.estimate.map(|e| format!(" (~{e}t)")).unwrap_or_default();
                println!("  next: {}{est}", t.slug);
            } else if complete == total && total > 0 {
                println!("  all tasks done");
            } else if blocked_count > 0 {
                println!("  no runnable tasks — see open questions below");
            }
        }
        Err(_) => {
            println!("  (no plan.md — run /driver:new to scaffold)");
        }
    }

    // == active claim ==
    println!("\n== active claim ==");
    match read_active(&root) {
        Some(c) => {
            let est = c
                .estimate_at_claim
                .map(|e| format!(", est ~{e}"))
                .unwrap_or_default();
            println!(
                "  {} — turn {}/{} (started {}{est})",
                c.slug, c.turn, c.max_turns, c.started_at
            );
        }
        None => println!("  none"),
    }

    // == recent ticks ==
    println!("\n== recent ticks ==");
    let history = read_history(&root);
    let recent: Vec<&HistoryRecord> = history.iter().rev().take(5).collect();
    if recent.is_empty() {
        println!("  none yet");
    } else {
        for r in recent {
            let turns = match (r.actual_turns, r.estimate) {
                (Some(a), Some(e)) => format!("{a}t / ~{e}t"),
                (Some(a), None) => format!("{a}t"),
                (None, Some(e)) => format!("(no claim) / ~{e}t"),
                (None, None) => "(no claim)".into(),
            };
            println!("  {} [{}]  {turns}", r.ts, r.slug);
        }
    }

    // == open questions ==
    println!("\n== open questions ==");
    let mut total_q = 0u32;
    let mut total_unanswered = 0u32;
    if let Ok((_, _, tasks)) = &plan {
        for task in tasks {
            let qs = read_questions(&root, &task.slug);
            if qs.is_empty() {
                continue;
            }
            let unanswered_here: Vec<&Question> = qs.iter().filter(|q| !q.answered).collect();
            total_q += qs.len() as u32;
            total_unanswered += unanswered_here.len() as u32;
            if unanswered_here.is_empty() {
                continue;
            }
            println!("  [{}]", task.slug);
            for q in &unanswered_here {
                let mut tags: Vec<String> = Vec::new();
                if let Some(r) = &q.rule {
                    tags.push(r.clone());
                }
                if q.halt {
                    tags.push("halt".into());
                }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", tags.join(", "))
                };
                let mark = if q.halt { "✗" } else { "·" };
                println!("    {mark} Q{}{tag_str}: {}", q.index, q.summary);
            }
        }
    }
    if total_q == 0 {
        println!("  none");
    } else if total_unanswered == 0 {
        println!("  all {total_q} answered — staged tasks can be re-ticked");
    }
    Ok(())
}

fn cmd_next(json: bool) -> Result<(), String> {
    let root = project_root()?;
    let (_, _, tasks) = read_plan(&root)?;
    let blockers = transitive_blockers(&root, &tasks);
    let next = tasks
        .iter()
        .find(|t| is_runnable(t, &tasks, &blockers))
        .ok_or_else(|| "no runnable tasks".to_string())?;
    if json {
        print!("{}", task_json(next));
    } else {
        print_task(next);
    }
    Ok(())
}

fn cmd_tasks() -> Result<(), String> {
    let root = project_root()?;
    let (_, _, tasks) = read_plan(&root)?;
    let blockers = transitive_blockers(&root, &tasks);
    for t in &tasks {
        let status = if t.done {
            "done   "
        } else if blockers.get(&t.slug).is_some_and(|b| !b.is_empty()) {
            "blocked"
        } else if is_runnable(t, &tasks, &blockers) {
            "runable"
        } else {
            "waiting"
        };
        let estimate = t
            .estimate
            .map(|e| format!(" ~{e}t"))
            .unwrap_or_default();
        let deps = if t.depends.is_empty() {
            String::new()
        } else {
            format!(" [depends: {}]", t.depends.join(", "))
        };
        println!("{status}  {}{estimate}{deps}", t.slug);
    }
    Ok(())
}

fn cmd_tick(slug: &str, done: bool) -> Result<(), String> {
    let root = project_root()?;
    let (path, mut lines, tasks) = read_plan(&root)?;
    let task = tasks
        .iter()
        .find(|t| t.slug == slug)
        .ok_or_else(|| format!("no task '{slug}'"))?;
    let estimate = task.estimate;
    // Enforce principles + answered-status when ticking to done.
    if done {
        let active = read_active(&root);
        let start_commit = active
            .as_ref()
            .filter(|c| c.slug == slug)
            .map(|c| c.start_commit.clone());
        enforce_tick_gates(&root, slug, start_commit.as_deref())?;
    }
    let line = &mut lines[task.line];
    let (from, to) = if done {
        ("- [ ]", "- [x]")
    } else {
        ("- [x]", "- [ ]")
    };
    if let Some(i) = line.find(from) {
        line.replace_range(i..i + 5, to);
        fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("write: {e}"))?;
        println!("{slug} → {}", if done { "done" } else { "open" });
        if done {
            let claim = read_active(&root).filter(|c| c.slug == slug);
            let actual_turns = claim.as_ref().map(|c| c.turn);
            let budget = claim.as_ref().map(|c| c.max_turns);
            let estimate_at_claim = claim.as_ref().and_then(|c| c.estimate_at_claim);
            append_history(
                &root,
                &HistoryRecord {
                    ts: iso_now(),
                    slug: slug.to_string(),
                    estimate,
                    actual_turns,
                    budget,
                    estimate_at_claim,
                    status: "done",
                },
            );
            match (claim.as_ref(), estimate) {
                (Some(c), Some(e)) => {
                    println!(
                        "  budget: {}/{} turns (est {e}, ratio {:.2})",
                        c.turn,
                        c.max_turns,
                        c.turn as f64 / e as f64
                    );
                }
                (Some(c), None) => {
                    println!("  budget: {}/{} turns (no estimate)", c.turn, c.max_turns);
                }
                (None, _) => {
                    println!(
                        "  note: no active claim — actual_turns not recorded in .history.jsonl. Use `driver claim` before working to capture calibration data."
                    );
                }
            }
            if claim.is_some() {
                delete_active(&root);
            }
        }
    } else {
        println!("No change ({slug} already {}).", if done { "done" } else { "open" });
    }
    Ok(())
}

fn cmd_rename(old: &str, new: &str) -> Result<(), String> {
    if old == new {
        return Err("old and new slug are identical".into());
    }
    let root = project_root()?;
    let (path, lines, tasks) = read_plan(&root)?;
    if !tasks.iter().any(|t| t.slug == old) {
        return Err(format!("no task '{old}'"));
    }
    if tasks.iter().any(|t| t.slug == new) {
        return Err(format!("task '{new}' already exists"));
    }
    // Rewrite plan.md: replace **old** with **new**, and update [depends:] lists.
    let new_lines: Vec<String> = lines
        .into_iter()
        .map(|l| {
            let l = l.replace(&format!("**{old}**"), &format!("**{new}**"));
            rewrite_depends(&l, old, new)
        })
        .collect();
    fs::write(&path, new_lines.join("\n") + "\n").map_err(|e| format!("write: {e}"))?;
    // Rename associated per-task files.
    for suffix in ["_design.md", "_questions.md"] {
        let from = root.join(format!("{old}{suffix}"));
        if from.exists() {
            let to = root.join(format!("{new}{suffix}"));
            fs::rename(&from, &to).map_err(|e| format!("rename {}: {e}", from.display()))?;
            println!("Renamed {} → {}", from.display(), to.display());
        }
    }
    println!("Renamed task {old} → {new}.");
    Ok(())
}

fn rewrite_depends(line: &str, old: &str, new: &str) -> String {
    let Some(i) = line.find("[depends:") else {
        return line.to_string();
    };
    let after = &line[i..];
    let Some(j) = after.find(']') else {
        return line.to_string();
    };
    let inner = &after[..j];
    let updated: String = inner
        .split(',')
        .map(|tok| {
            let trimmed = tok.trim_start_matches("[depends:").trim();
            let prefix = if tok.trim_start().starts_with("[depends:") {
                "[depends: "
            } else {
                ""
            };
            let replaced = if trimmed == old { new } else { trimmed };
            format!("{prefix}{replaced}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}{updated}{}", &line[..i], &line[i + j..])
}

// ---- helpers ----

fn print_task(t: &Task) {
    let estimate = t
        .estimate
        .map(|e| format!(" (~{e}t)"))
        .unwrap_or_default();
    let deps = if t.depends.is_empty() {
        String::new()
    } else {
        format!(" [depends: {}]", t.depends.join(", "))
    };
    println!("{}{estimate}{deps}", t.slug);
    if !t.description.is_empty() {
        for line in textwrap_simple(&t.description, 76) {
            println!("  {line}");
        }
    }
}

fn textwrap_simple(s: &str, w: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in s.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= w {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn task_json(t: &Task) -> String {
    let mut s = String::from("{\n");
    s.push_str(&format!("  \"slug\": {},\n", json_str(&t.slug)));
    if let Some(e) = t.estimate {
        s.push_str(&format!("  \"estimate\": {e},\n"));
    }
    s.push_str("  \"depends\": [");
    for (i, d) in t.depends.iter().enumerate() {
        let sep = if i + 1 == t.depends.len() { "" } else { ", " };
        s.push_str(&json_str(d));
        s.push_str(sep);
    }
    s.push_str("],\n");
    s.push_str(&format!(
        "  \"description\": {}\n",
        json_str(&t.description)
    ));
    s.push_str("}\n");
    s
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- claim / release / gate ----
//
// The "claim" mechanism is Driver's analogue of `/goal`. A claim records
// {track, slug, max_turns, turn count} in `driver/.active`. The Stop
// hook (snippet printed by `driver doctor` when missing) runs `driver gate` after
// every agent turn. The gate increments the turn counter, releases the
// claim if the task has been ticked or blocked or budget exhausted, and
// otherwise exits 2 to prevent the agent from stopping.

#[derive(Debug, Clone)]
struct ActiveClaim {
    slug: String,
    max_turns: u32,
    turn: u32,
    started_at: String,
    /// Git HEAD commit at claim time, or empty if not in a git repo. Used by
    /// `driver tick` to diff what changed during the task against the
    /// principles.md rules.
    start_commit: String,
    /// Plan estimate (~K turns) at claim time. Captured here so tick can
    /// record the estimate the *user authored* before any mid-flight
    /// plan.md edits, and so `driver stats` can flag scope-growth.
    estimate_at_claim: Option<u32>,
}

fn active_path(root: &Path) -> PathBuf {
    root.join(".active")
}

fn read_active(root: &Path) -> Option<ActiveClaim> {
    let path = active_path(root);
    let text = fs::read_to_string(&path).ok()?;
    let mut slug = String::new();
    let mut max_turns: u32 = 50;
    let mut turn: u32 = 0;
    let mut started_at = String::new();
    let mut start_commit = String::new();
    let mut estimate_at_claim: Option<u32> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "slug" => slug = v.trim().to_string(),
            "max_turns" => max_turns = v.trim().parse().unwrap_or(50),
            "turn" => turn = v.trim().parse().unwrap_or(0),
            "started_at" => started_at = v.trim().to_string(),
            "start_commit" => start_commit = v.trim().to_string(),
            "estimate_at_claim" => estimate_at_claim = v.trim().parse().ok(),
            // Legacy `track=` field from pre-flat data is silently ignored.
            _ => {}
        }
    }
    if slug.is_empty() {
        return None;
    }
    Some(ActiveClaim {
        slug,
        max_turns,
        turn,
        started_at,
        start_commit,
        estimate_at_claim,
    })
}

fn write_active(root: &Path, claim: &ActiveClaim) -> Result<(), String> {
    let path = active_path(root);
    let est = claim
        .estimate_at_claim
        .map(|e| e.to_string())
        .unwrap_or_default();
    let body = format!(
        "slug={}\nmax_turns={}\nturn={}\nstarted_at={}\nstart_commit={}\nestimate_at_claim={}\n",
        claim.slug,
        claim.max_turns,
        claim.turn,
        claim.started_at,
        claim.start_commit,
        est,
    );
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

fn delete_active(root: &Path) {
    let _ = fs::remove_file(active_path(root));
}

fn iso_now() -> String {
    // Avoid pulling in chrono. Use the system clock and format as
    // best-effort ISO 8601 UTC.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // YYYY-MM-DDTHH:MM:SSZ — convert secs to a date.
    // Naive: use a minimal algorithm.
    format_unix_seconds(secs)
}

fn format_unix_seconds(mut secs: u64) -> String {
    // Days since 1970-01-01.
    let day_secs = 86_400u64;
    let days = (secs / day_secs) as i64;
    let time_in_day = secs % day_secs;
    let hh = time_in_day / 3600;
    let mm = (time_in_day % 3600) / 60;
    let ss = time_in_day % 60;
    // Compute Y-M-D from days since epoch (1970-01-01 was Thursday).
    let (y, mo, d) = days_to_ymd(days + 719_468); // shift so day 0 = 0000-03-01
    let _ = &mut secs;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, hh, mm, ss)
}

// Adapted from Howard Hinnant's date algorithms (public domain).
fn days_to_ymd(z: i64) -> (i64, u32, u32) {
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn cmd_claim(slug: &str, max_turns: Option<u32>) -> Result<(), String> {
    let root = project_root()?;
    let (_, _, tasks) = read_plan(&root)?;
    let task = tasks
        .iter()
        .find(|t| t.slug == slug)
        .ok_or_else(|| format!("no task '{slug}'"))?;
    if task.done {
        return Err(format!("task '{slug}' is already done"));
    }
    if let Some(existing) = read_active(&root) {
        return Err(format!(
            "claim already active for {} (turn {}/{}). Release first with `driver release` if you want to switch.",
            existing.slug, existing.turn, existing.max_turns
        ));
    }
    let estimate_at_claim = task.estimate;
    // Default budget: ceil(estimate * 1.3 / 5) * 5, or 50 if no estimate.
    let (max_turns, defaulted) = match max_turns {
        Some(n) => (n, false),
        None => {
            let n = estimate_at_claim
                .map(|e| {
                    let raw = (e as f64 * 1.3).ceil() as u32;
                    raw.div_ceil(5) * 5
                })
                .unwrap_or(50);
            (n, true)
        }
    };
    let start_commit = git_head_commit(&root).unwrap_or_default();
    let claim = ActiveClaim {
        slug: slug.to_string(),
        max_turns,
        turn: 0,
        started_at: iso_now(),
        start_commit,
        estimate_at_claim,
    };
    write_active(&root, &claim)?;
    let how = if defaulted {
        match estimate_at_claim {
            Some(e) => format!(" (default: ceil(~{e} × 1.3 / 5) × 5)"),
            None => " (default: no estimate)".to_string(),
        }
    } else {
        String::new()
    };
    println!("Claimed {} for up to {max_turns} turns{how}.", claim.slug);
    if let Some(e) = estimate_at_claim {
        if !defaulted && max_turns > (e as f64 * 1.5) as u32 {
            println!(
                "  note: budget {max_turns} is {:.1}× plan estimate (~{e}). Scope-growth will be flagged in `driver stats`.",
                max_turns as f64 / e as f64
            );
        }
    }
    Ok(())
}

fn cmd_release() -> Result<(), String> {
    let root = match find_driver_root() {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let prev = read_active(&root);
    delete_active(&root);
    match prev {
        Some(c) => println!("Released {}.", c.slug),
        None => println!("No active claim."),
    }
    Ok(())
}

/// The Stop-hook callback.
/// Exit codes:
///   0 — allow the agent to stop (no claim, or claim is satisfied).
///   2 — block the stop (claim is still incomplete; agent must continue).
fn cmd_gate() -> ExitCode {
    // No driver/ directory in scope → silently allow stop.
    let root = match find_driver_root() {
        Ok(r) => r,
        Err(_) => return ExitCode::SUCCESS,
    };
    let Some(mut claim) = read_active(&root) else {
        return ExitCode::SUCCESS;
    };
    claim.turn += 1;

    // Check whether the task is now done.
    if let Ok((_, _, tasks)) = read_plan(&root) {
        if let Some(task) = tasks.iter().find(|t| t.slug == claim.slug) {
            if task.done {
                delete_active(&root);
                return ExitCode::SUCCESS;
            }
        } else {
            // Task slug no longer exists (renamed or removed). Release.
            delete_active(&root);
            return ExitCode::SUCCESS;
        }
    }

    // Check whether the task is halted by an unanswered halt-question.
    if directly_blocked(&root, &claim.slug) {
        delete_active(&root);
        return ExitCode::SUCCESS;
    }

    // Budget check.
    if claim.turn > claim.max_turns {
        delete_active(&root);
        eprintln!(
            "driver gate: turn budget exceeded for {} ({}/{}). Released claim — agent may stop.",
            claim.slug, claim.turn, claim.max_turns
        );
        return ExitCode::SUCCESS;
    }

    // Persist the bumped turn counter.
    if let Err(e) = write_active(&root, &claim) {
        eprintln!("driver gate: failed to update claim: {e}");
        return ExitCode::SUCCESS;
    }

    // Block the stop. Lead with the pacing info so the agent sees it
    // at a glance every turn.
    let pct = (claim.turn as f64) / (claim.max_turns as f64);
    let warn = if pct >= 0.90 {
        " ⚠ NEAR BUDGET"
    } else if pct >= 0.75 {
        " ⚠ 75%+"
    } else {
        ""
    };
    eprintln!(
        "[driver] turn {turn}/{max} — {slug}{warn}. Tick when done or `driver ask {slug} --halt \"<question>\"`.",
        slug = claim.slug,
        turn = claim.turn,
        max = claim.max_turns,
    );
    ExitCode::from(2)
}

// ---- history (.history.jsonl) + stats ----

struct HistoryRecord {
    ts: String,
    slug: String,
    /// Plan estimate at tick time. Captured by re-reading plan.md;
    /// reflects whatever value plan.md holds when tick runs.
    estimate: Option<u32>,
    /// Turns actually spent on the task. `None` when tick fired without
    /// a matching active claim (no gate was counting turns), so the
    /// duration is unknown but the completion is still recorded.
    actual_turns: Option<u32>,
    /// Claim budget (max_turns) at the time the claim was opened.
    /// `None` when ticked without a claim.
    budget: Option<u32>,
    /// Plan estimate snapshotted at claim time, before any mid-flight
    /// edits to plan.md. Used by stats to detect scope-growth even when
    /// the user bumped the estimate during the run.
    estimate_at_claim: Option<u32>,
    status: &'static str,
}

fn history_path(root: &Path) -> PathBuf {
    root.join(".history.jsonl")
}

fn append_history(root: &Path, rec: &HistoryRecord) {
    use std::io::Write as _;
    let path = history_path(root);
    let opt_num = |v: Option<u32>| {
        v.map(|n| n.to_string()).unwrap_or_else(|| "null".into())
    };
    let line = format!(
        "{{\"ts\": {}, \"slug\": {}, \"estimate\": {}, \"actual_turns\": {}, \"budget\": {}, \"estimate_at_claim\": {}, \"status\": {}}}\n",
        json_str(&rec.ts),
        json_str(&rec.slug),
        opt_num(rec.estimate),
        opt_num(rec.actual_turns),
        opt_num(rec.budget),
        opt_num(rec.estimate_at_claim),
        json_str(rec.status),
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn read_history(root: &Path) -> Vec<HistoryRecord> {
    let path = history_path(root);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // Minimal JSON parse: pull the fields we wrote. Robust enough
        // for our own append-only format.
        let ts = json_field(line, "ts").unwrap_or_default();
        let slug = json_field(line, "slug").unwrap_or_default();
        let estimate = json_number(line, "estimate");
        let actual_turns = json_number(line, "actual_turns");
        let budget = json_number(line, "budget");
        let estimate_at_claim = json_number(line, "estimate_at_claim");
        let status_owned = json_field(line, "status").unwrap_or_else(|| "done".into());
        let status: &'static str = match status_owned.as_str() {
            "blocked" => "blocked",
            _ => "done",
        };
        // Legacy records carried a `track` field; it's read and ignored.
        out.push(HistoryRecord {
            ts,
            slug,
            estimate,
            actual_turns,
            budget,
            estimate_at_claim,
            status,
        });
    }
    out
}

fn json_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let i = line.find(&needle)?;
    let rest = line[i + needle.len()..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_number(line: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\":");
    let i = line.find(&needle)?;
    let rest = line[i + needle.len()..].trim_start();
    // Stop at comma or `}`.
    let end = rest
        .find(|c: char| c == ',' || c == '}')
        .unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn cmd_stats() -> Result<(), String> {
    let root = find_driver_root()?;
    let records = read_history(&root);
    let mut records: Vec<&HistoryRecord> = records
        .iter()
        .filter(|r| r.status == "done")
        .collect();
    if records.is_empty() {
        println!("No history yet. Tick a task to populate .history.jsonl.");
        return Ok(());
    }
    records.sort_by_key(|r| r.ts.clone());

    let n = records.len();
    let n_no_actual = records.iter().filter(|r| r.actual_turns.is_none()).count();
    let with_actual: Vec<(&HistoryRecord, u32)> = records
        .iter()
        .filter_map(|r| r.actual_turns.map(|a| (*r, a)))
        .collect();

    println!("Driver stats — {n} completed task(s)");
    if n_no_actual > 0 {
        println!(
            "  ({n_no_actual} ticked without a claim — actual_turns not captured)"
        );
    }

    if !with_actual.is_empty() {
        let actuals: Vec<u32> = with_actual.iter().map(|(_, a)| *a).collect();
        let total: u32 = actuals.iter().sum();
        let mean = total as f64 / actuals.len() as f64;
        let mut sorted = actuals.clone();
        sorted.sort();
        let m = sorted.len();
        let median = if m % 2 == 1 {
            sorted[m / 2] as f64
        } else {
            (sorted[m / 2 - 1] + sorted[m / 2]) as f64 / 2.0
        };
        println!("  total turns:   {total}");
        println!("  mean turns:    {mean:.1}");
        println!("  median turns:  {median:.1}");

        let with_est: Vec<(&HistoryRecord, u32, u32)> = with_actual
            .iter()
            .filter_map(|(r, a)| r.estimate.map(|e| (*r, *a, e)))
            .collect();
        if !with_est.is_empty() {
            let ratio_sum: f64 = with_est
                .iter()
                .map(|(_, a, e)| *a as f64 / *e as f64)
                .sum();
            let ratio_mean = ratio_sum / with_est.len() as f64;
            println!(
                "  actual/est:    mean {ratio_mean:.2} ({})",
                if ratio_mean > 1.10 {
                    "estimates run light"
                } else if ratio_mean < 0.90 {
                    "estimates run heavy"
                } else {
                    "well-calibrated"
                }
            );

            let mut by_ratio: Vec<(&HistoryRecord, u32, u32, f64)> = with_est
                .iter()
                .map(|(r, a, e)| (*r, *a, *e, *a as f64 / *e as f64))
                .collect();
            by_ratio.sort_by(|x, y| y.3.partial_cmp(&x.3).unwrap_or(std::cmp::Ordering::Equal));
            let under: Vec<_> = by_ratio.iter().take(3).collect();
            let over: Vec<_> = by_ratio.iter().rev().take(3).collect();
            if !under.is_empty() {
                println!("\nLargest under-estimates (actual ≫ est):");
                for (r, a, e, ratio) in under {
                    println!(
                        "  {slug:<28} ~{e}t est, {a}t actual ({ratio:.2}×)",
                        slug = r.slug,
                    );
                }
            }
            if !over.is_empty() {
                println!("\nLargest over-estimates (actual ≪ est):");
                for (r, a, e, ratio) in over {
                    println!(
                        "  {slug:<28} ~{e}t est, {a}t actual ({ratio:.2}×)",
                        slug = r.slug,
                    );
                }
            }
        }

        // Scope-growth: claim budget exceeded the at-claim plan estimate
        // by ≥1.5×, or actual exceeded the at-claim estimate by ≥1.5×.
        // Catches mid-flight estimate bumps even when the plan was
        // edited so `estimate` and `estimate_at_claim` diverged.
        let grew: Vec<(&HistoryRecord, u32, u32, u32)> = with_actual
            .iter()
            .filter_map(|(r, a)| {
                let e0 = r.estimate_at_claim?;
                let b = r.budget?;
                let ratio_b = b as f64 / e0 as f64;
                let ratio_a = *a as f64 / e0 as f64;
                if ratio_b >= 1.5 || ratio_a >= 1.5 {
                    Some((*r, *a, b, e0))
                } else {
                    None
                }
            })
            .collect();
        if !grew.is_empty() {
            println!("\nScope-growth (budget or actual ≥ 1.5× plan estimate at claim):");
            for (r, a, b, e0) in &grew {
                println!(
                    "  {slug:<28} ~{e0}t planned → {b}t budgeted, {a}t actual",
                    slug = r.slug,
                );
            }
        }
    }
    Ok(())
}

// ---- doctor ----

fn cmd_doctor() -> Result<(), String> {
    let mut all_ok = true;
    let bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "driver".into());

    // 1. Binary discoverable on PATH as `driver`?
    let on_path = which_on_path("driver").is_some();
    report(on_path, "`driver` on PATH", &if on_path {
        which_on_path("driver").unwrap()
    } else {
        format!("not found on PATH (binary lives at {bin})")
    });
    all_ok &= on_path;

    // 2. Stop hook installed in ~/.claude/settings.json?
    let settings_text = read_settings();
    let (hook_ok, hook_detail) = check_stop_hook(&bin, settings_text.as_deref());
    report(hook_ok, "Stop hook installed", &hook_detail);
    all_ok &= hook_ok;
    if !hook_ok {
        // Print the snippet inline so the user can paste it directly.
        println!();
        println!("  Paste this into ~/.claude/settings.json:");
        for line in stop_hook_snippet(&bin).lines() {
            println!("    {line}");
        }
        println!();
        println!("  Merge into an existing `hooks` section if you have one.");
    } else {
        // 2b. Actually invoke the hook's command to verify it works.
        // Text-matching `driver gate` in settings.json doesn't catch a
        // stale binary path, missing executable bit, or a hook pointing
        // at a different driver build that errors out. Run the command
        // from a no-project cwd (so gate finds no .active and exits 0)
        // and check the exit code.
        let cmd = settings_text
            .as_deref()
            .and_then(extract_hook_command);
        match cmd {
            Some(c) => match invoke_hook_command(&c) {
                Ok(0) => report(true, "Stop hook runs", &format!("`{c}` exit 0")),
                Ok(n) => {
                    report(
                        false,
                        "Stop hook runs",
                        &format!("`{c}` exited {n} (expected 0 from a no-claim state)"),
                    );
                    all_ok = false;
                }
                Err(e) => {
                    report(false, "Stop hook runs", &format!("`{c}` failed to run: {e}"));
                    all_ok = false;
                }
            },
            None => {
                report(
                    false,
                    "Stop hook runs",
                    "couldn't extract `command` field from settings.json",
                );
                all_ok = false;
            }
        }
    }

    // 3. cwd inside a driver project? Informational — doctor passes
    // for global setup even without a project. Mark with ℹ when absent.
    match find_driver_root() {
        Ok(root) => {
            report(true, "driver/ project in cwd", &root.display().to_string());
            if let Some(c) = read_active(&root) {
                report(
                    true,
                    "Active claim",
                    &format!("{} — turn {}/{}", c.slug, c.turn, c.max_turns),
                );
            } else {
                report(true, "Active claim", "(none)");
            }
        }
        Err(_) => {
            println!("ℹ {:<28} (cd into a project to check)", "driver/ project in cwd");
        }
    }

    println!();
    if all_ok {
        println!("✓ Driver setup looks good.");
    } else {
        println!("✗ Driver setup has issues. Fix the ✗ items above.");
        return Err("driver doctor reported problems".into());
    }
    Ok(())
}

fn which_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Read the user's Claude Code settings.json, if it exists.
fn read_settings() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".claude").join("settings.json");
    fs::read_to_string(&path).ok()
}

fn settings_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".claude").join("settings.json")
}

/// Pull out the `"command"` value of the Stop hook entry, if present.
/// Naive scan: finds any `"command": "..."` whose value ends with
/// ` gate`. Good enough for our own snippet shape; doesn't try to
/// understand the full JSON structure.
fn extract_hook_command(settings: &str) -> Option<String> {
    let needle = "\"command\":";
    let mut pos = 0usize;
    while let Some(rel) = settings[pos..].find(needle) {
        let after = pos + rel + needle.len();
        let rest = settings[after..].trim_start();
        let bytes_skipped = settings[after..].len() - rest.len();
        if !rest.starts_with('"') {
            pos = after + bytes_skipped + 1;
            continue;
        }
        let val_start = after + bytes_skipped + 1;
        // Find closing quote. (No escape handling — our snippet
        // doesn't contain quotes inside the command string.)
        if let Some(end_rel) = settings[val_start..].find('"') {
            let val = &settings[val_start..val_start + end_rel];
            if val.ends_with(" gate") {
                return Some(val.to_string());
            }
            pos = val_start + end_rel + 1;
        } else {
            return None;
        }
    }
    None
}

/// Invoke the hook's command from a neutral cwd (the system temp dir,
/// where `gate` will find no driver project and exit 0). Returns the
/// exit code, or an Err describing why the process couldn't be spawned.
fn invoke_hook_command(command: &str) -> Result<i32, String> {
    let temp = std::env::temp_dir();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&temp)
        .output()
        .map_err(|e| format!("spawn: {e}"))?;
    Ok(output.status.code().unwrap_or(-1))
}

fn check_stop_hook(bin: &str, settings: Option<&str>) -> (bool, String) {
    let Some(text) = settings else {
        return (false, format!("{} not found", settings_path().display()));
    };
    let path = settings_path();
    // Naive check: does the file mention `<bin> gate` or any `driver gate` command in a Stop hook context?
    let needles = [format!("{bin} gate"), "driver gate".to_string()];
    let mentions_bin = needles.iter().any(|n| text.contains(n.as_str()));
    let mentions_stop = text.contains("\"Stop\"");
    if mentions_bin && mentions_stop {
        (true, format!("{}", path.display()))
    } else if mentions_stop {
        (
            false,
            format!(
                "{} has a Stop hook but no `driver gate` command. Did the binary path change?",
                path.display()
            ),
        )
    } else {
        (
            false,
            format!("{} has no Stop hook entry for driver", path.display()),
        )
    }
}

fn report(ok: bool, label: &str, detail: &str) {
    let mark = if ok { "✓" } else { "✗" };
    println!("{mark} {label:<28} {detail}");
}

fn stop_hook_snippet(bin: &str) -> String {
    format!(
        r#"{{
  "hooks": {{
    "Stop": [
      {{
        "matcher": "",
        "hooks": [
          {{ "type": "command", "command": "{bin} gate", "timeout": 10000 }}
        ]
      }}
    ]
  }}
}}"#
    )
}

// ---- principles + ask/questions + tick enforcement ----

#[derive(Debug, Clone)]
struct PrincipleRule {
    name: String,
    glob: String,
    #[allow(dead_code)]
    description: String,
}

fn principles_path(root: &Path) -> PathBuf {
    root.join("principles.md")
}

/// Parse `driver/principles.md`. Looks for one block per rule with the shape:
///
///     - name: <slug>
///       glob: <glob-pattern>
///       description: <one-line>
///
/// Anywhere else (free prose, "guidance" sections, etc.) is ignored. The
/// parser is intentionally lenient — if there are no rules or the file
/// is absent, we return an empty list and the floor is effectively off.
fn read_principles(root: &Path) -> Vec<PrincipleRule> {
    let text = match fs::read_to_string(principles_path(root)) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut rules: Vec<PrincipleRule> = Vec::new();
    let mut cur_name: Option<String> = None;
    let mut cur_glob: Option<String> = None;
    let mut cur_desc: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(v) = line.strip_prefix("- name:") {
            // Flush previous rule, start new.
            if let (Some(n), Some(g)) = (cur_name.take(), cur_glob.take()) {
                rules.push(PrincipleRule {
                    name: n,
                    glob: g,
                    description: cur_desc.take().unwrap_or_default(),
                });
            }
            cur_name = Some(v.trim().to_string());
            cur_glob = None;
            cur_desc = None;
        } else if let Some(v) = line.strip_prefix("glob:") {
            cur_glob = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            cur_desc = Some(v.trim().to_string());
        }
    }
    if let (Some(n), Some(g)) = (cur_name, cur_glob) {
        rules.push(PrincipleRule {
            name: n,
            glob: g,
            description: cur_desc.unwrap_or_default(),
        });
    }
    rules
}

/// Glob matcher with a single `*` wildcard. `*` matches any run of
/// characters that doesn't include `/`. Multi-`*` and `**` are not
/// supported in v1 — one wildcard per glob covers the common cases
/// (exact files, `dir/*.ext`).
fn matches_glob(path: &str, glob: &str) -> bool {
    if let Some((prefix, suffix)) = glob.split_once('*') {
        if !path.starts_with(prefix) {
            return false;
        }
        if !path.ends_with(suffix) {
            return false;
        }
        if path.len() < prefix.len() + suffix.len() {
            return false;
        }
        let middle = &path[prefix.len()..path.len() - suffix.len()];
        !middle.contains('/')
    } else {
        path == glob
    }
}

fn git_head_commit(root: &Path) -> Option<String> {
    let project_root = root.parent()?; // strip the trailing "driver/"
    let out = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(project_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn git_touched_files(root: &Path, since_commit: &str) -> Vec<String> {
    let Some(project_root) = root.parent() else {
        return Vec::new();
    };
    let range = format!("{since_commit}..HEAD");
    let out = match std::process::Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .arg(&range)
        .current_dir(project_root)
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn questions_path(root: &Path, slug: &str) -> PathBuf {
    root.join(format!("{slug}_questions.md"))
}

#[derive(Debug, Clone)]
struct Question {
    /// 1-based index within the file.
    index: u32,
    summary: String,
    rule: Option<String>,
    context: String,
    answered: bool,
    /// Marks the question as a hard halt: the task can't make further
    /// progress until this is answered. An unanswered halt-question
    /// makes the task act blocked (`is_runnable` returns false; gate
    /// releases the claim; dependents are transitively unreachable).
    halt: bool,
}

fn read_questions(root: &Path, slug: &str) -> Vec<Question> {
    let path = questions_path(root, slug);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<Question> = Vec::new();
    let mut cur_summary: Option<String> = None;
    let mut cur_rule: Option<String> = None;
    let mut cur_context = String::new();
    let mut cur_answered = false;
    let mut cur_halt = false;
    let mut idx: u32 = 0;
    for raw in text.lines() {
        if let Some(rest) = raw.strip_prefix("## Q") {
            // Flush previous.
            if let Some(s) = cur_summary.take() {
                out.push(Question {
                    index: idx,
                    summary: s,
                    rule: cur_rule.take(),
                    context: std::mem::take(&mut cur_context).trim().to_string(),
                    answered: cur_answered,
                    halt: cur_halt,
                });
            }
            cur_answered = false;
            cur_halt = false;
            cur_rule = None;
            cur_context.clear();
            // Parse "<n>: <summary>" or "<n>".
            let rest = rest.trim_start();
            let (num_part, after) = match rest.split_once(':') {
                Some((n, a)) => (n.trim(), a.trim()),
                None => (rest, ""),
            };
            idx = num_part.parse().unwrap_or(idx + 1);
            cur_summary = Some(after.to_string());
        } else if let Some(v) = raw.strip_prefix("**rule:**") {
            let v = v.trim();
            if !v.is_empty() && v != "(self-classified)" {
                cur_rule = Some(v.to_string());
            }
        } else if let Some(v) = raw.strip_prefix("**answer:**") {
            let v = v.trim();
            cur_answered = !(v.is_empty() || v == "_pending_");
        } else if let Some(v) = raw.strip_prefix("**halt:**") {
            cur_halt = matches!(v.trim(), "true" | "yes" | "1");
        } else if let Some(v) = raw.strip_prefix("**context:**") {
            cur_context.push_str(v.trim());
            cur_context.push('\n');
        } else if cur_summary.is_some()
            && !raw.starts_with("# ")
            && !raw.starts_with("## ")
            && !raw.trim().is_empty()
        {
            // Continuation of context.
            if !cur_context.is_empty() {
                cur_context.push(' ');
            }
            cur_context.push_str(raw.trim());
        }
    }
    if let Some(s) = cur_summary {
        out.push(Question {
            index: idx,
            summary: s,
            rule: cur_rule,
            context: cur_context.trim().to_string(),
            answered: cur_answered,
            halt: cur_halt,
        });
    }
    out
}

fn cmd_ask(
    slug: &str,
    question: &str,
    rule: Option<&str>,
    context: Option<&str>,
    halt: bool,
) -> Result<(), String> {
    let root = project_root()?;
    let (_, _, tasks) = read_plan(&root)?;
    if !tasks.iter().any(|t| t.slug == slug) {
        return Err(format!("no task '{slug}'"));
    }
    // Validate the rule name if given.
    if let Some(r) = rule {
        let rules = read_principles(&root);
        if !rules.iter().any(|p| p.name == r) {
            let known: Vec<String> = rules.iter().map(|p| p.name.clone()).collect();
            return Err(format!(
                "unknown principles rule '{r}'. Known: [{}]. Add it to driver/principles.md or omit --rule.",
                known.join(", ")
            ));
        }
    }
    let existing = read_questions(&root, slug);
    let next_idx = existing.iter().map(|q| q.index).max().unwrap_or(0) + 1;
    let path = questions_path(&root, slug);
    let mut body = if path.exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        format!("# Open questions — {slug}\n\nAnswer each by replacing `_pending_` with your decision; re-run `/driver:go` or `/driver:do` to resume.\n")
    };
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&format!("## Q{next_idx}: {}\n", question.trim()));
    body.push_str(&format!(
        "**rule:** {}\n",
        rule.unwrap_or("(self-classified)")
    ));
    if halt {
        body.push_str("**halt:** true\n");
    }
    if let Some(c) = context {
        body.push_str(&format!("**context:** {}\n", c.trim()));
    }
    body.push_str("**answer:** _pending_\n");
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    let mut tags: Vec<String> = Vec::new();
    if let Some(r) = rule {
        tags.push(format!("rule={r}"));
    }
    if halt {
        tags.push("halt".into());
    }
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    };
    println!("Q{next_idx} logged for {slug}{tag_str} → {}", path.display());
    if halt {
        // If this halt-question belongs to the actively-claimed task,
        // release the claim — the agent has signalled "stuck."
        if let Some(claim) = read_active(&root) {
            if claim.slug == slug {
                delete_active(&root);
                println!("  claim released (halt question logged).");
            }
        }
    }
    Ok(())
}

/// Record an answer to question <index> in <slug>_questions.md.
/// Walks the file line-by-line, finds the matching `## Q<index>:`
/// section, and replaces its `**answer:** ...` line (or the first
/// `**answer:**` line within that section). Idempotent — running
/// twice replaces the prior answer.
fn cmd_answer(slug: &str, index: u32, answer: &str) -> Result<(), String> {
    let root = project_root()?;
    let path = questions_path(&root, slug);
    if !path.exists() {
        return Err(format!("no questions file for {slug}"));
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
    let header_prefix = format!("## Q{index}");
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut replaced = false;
    for line in text.lines() {
        if line.starts_with("## Q") {
            // Entering a new Q section.
            in_section = line.starts_with(&header_prefix)
                && line
                    .chars()
                    .nth(header_prefix.len())
                    .map(|c| c == ':' || c.is_whitespace())
                    .unwrap_or(true);
        }
        if in_section && !replaced && line.starts_with("**answer:**") {
            out.push(format!("**answer:** {}", answer.trim()));
            replaced = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !replaced {
        return Err(format!(
            "no Q{index} found in {} (or it has no `**answer:**` line)",
            path.display()
        ));
    }
    let mut new_text = out.join("\n");
    if text.ends_with('\n') {
        new_text.push('\n');
    }
    fs::write(&path, new_text).map_err(|e| format!("write: {e}"))?;
    println!("Q{index} answered for {slug}.");
    Ok(())
}

fn cmd_questions() -> Result<(), String> {
    let root = project_root()?;
    let (_, _, tasks) = read_plan(&root)?;
    let mut total = 0u32;
    let mut total_unanswered = 0u32;
    for task in &tasks {
        let qs = read_questions(&root, &task.slug);
        if qs.is_empty() {
            continue;
        }
        println!("[{}]", task.slug);
        for q in &qs {
            total += 1;
            let mark = if q.answered {
                "✓"
            } else if q.halt {
                "✗"
            } else {
                "·"
            };
            if !q.answered {
                total_unanswered += 1;
            }
            let mut tags: Vec<String> = Vec::new();
            if let Some(r) = &q.rule {
                tags.push(r.clone());
            }
            if q.halt {
                tags.push("halt".into());
            }
            let tag_str = if tags.is_empty() {
                String::new()
            } else {
                format!(" ({})", tags.join(", "))
            };
            println!("  {mark} Q{}{tag_str}: {}", q.index, q.summary);
            if !q.context.is_empty() {
                for line in textwrap_simple(&q.context, 72) {
                    println!("      {line}");
                }
            }
        }
    }
    if total == 0 {
        println!("No open questions.");
    } else {
        println!("\n{total} question(s) total, {total_unanswered} unanswered.");
    }
    Ok(())
}

/// Check the principles floor + answered-status for a task that's
/// about to be ticked. Returns Err with a diagnostic if anything fails.
fn enforce_tick_gates(
    root: &Path,
    slug: &str,
    start_commit: Option<&str>,
) -> Result<(), String> {
    // Floor: triggered files touched since claim start must have a
    // matching rule-tagged question.
    let rules = read_principles(root);
    let questions = read_questions(root, slug);
    if !rules.is_empty() {
        if let Some(commit) = start_commit.filter(|s| !s.is_empty()) {
            let touched = git_touched_files(root, commit);
            for rule in &rules {
                let tripped: Vec<&String> = touched
                    .iter()
                    .filter(|f| matches_glob(f, &rule.glob))
                    .collect();
                if tripped.is_empty() {
                    continue;
                }
                let has_q = questions
                    .iter()
                    .any(|q| q.rule.as_deref() == Some(rule.name.as_str()));
                if !has_q {
                    return Err(format!(
                        "principles rule '{}' tripped — touched: {}\n  → run `driver ask {slug} --rule {} \"<question>\" --context \"...\"` first, or revert the change.",
                        rule.name,
                        tripped.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                        rule.name,
                    ));
                }
            }
        }
    }
    // Answered: no question may be _pending_ at tick time.
    let unanswered: Vec<&Question> = questions.iter().filter(|q| !q.answered).collect();
    if !unanswered.is_empty() {
        let preview: Vec<String> = unanswered
            .iter()
            .take(3)
            .map(|q| format!("Q{}: {}", q.index, q.summary))
            .collect();
        return Err(format!(
            "{} unanswered question(s) in {slug}_questions.md:\n  {}\n  → answer them (replace `_pending_`) and re-run `driver tick`.",
            unanswered.len(),
            preview.join("\n  "),
        ));
    }
    Ok(())
}
