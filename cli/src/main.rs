// driver — minimal track planner.
//
// Operates on files under driver/ in the current project. Discovers the
// project root by walking up from the cwd looking for driver/tracks.md.
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
    /// Show all tracks and their next runnable task per track.
    Status,
    /// Print the next runnable task of a track.
    Next {
        track: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List all currently-runnable tasks of a track.
    Runnable {
        track: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List all tasks in a track with their status.
    Tasks { track: Option<String> },
    /// List blocked tasks of a track.
    Blocked { track: Option<String> },
    /// Tick a task as done.
    Tick { track: String, slug: String },
    /// Untick a task (mark open again).
    Untick { track: String, slug: String },
    /// Block a task by writing <slug>_blocked.md with the question.
    Block {
        track: String,
        slug: String,
        question: String,
    },
    /// Remove <slug>_blocked.md.
    Unblock { track: String, slug: String },
    /// Rename a task: updates plan.md and renames <slug>_* files.
    Rename {
        track: String,
        old_slug: String,
        new_slug: String,
    },
    /// Close a track if every task is ticked.
    Close { track: String },
    /// Print decisions.md for the track.
    Decisions { track: String },
    /// Start a claim on a task. Configures the Stop hook to keep the
    /// agent working until the task is ticked, blocked, or budget is hit.
    Claim {
        track: String,
        slug: String,
        #[arg(long, default_value_t = 50)]
        max_turns: u32,
    },
    /// End the current claim (idempotent).
    Release,
    /// Show the current claim, if any.
    ClaimStatus,
    /// Stop-hook callback. Exits 2 if the active claim is incomplete.
    Gate,
    /// Print the settings.json snippet for the Driver Stop hook.
    InitHook,
    /// Verify the local Driver setup (binary on PATH, hook installed, project layout).
    Doctor,
    /// Show estimate-vs-actual statistics from driver/.history.jsonl.
    Stats { track: Option<String> },
    /// Append an open question to <slug>_questions.md. Does not halt the
    /// task — agent can keep working on parts that don't depend on the
    /// answer. Use `driver block` for "fully stuck".
    Ask {
        track: String,
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
    },
    /// List open questions across the project (or one track).
    Questions { track: Option<String> },
    /// Record an answer to a logged question. Replaces `_pending_` on
    /// the matching Q's answer line.
    Answer {
        track: String,
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
        Cmd::Next { track, json } => cmd_next(track, json),
        Cmd::Runnable { track, json } => cmd_runnable(track, json),
        Cmd::Tasks { track } => cmd_tasks(track),
        Cmd::Blocked { track } => cmd_blocked(track),
        Cmd::Tick { track, slug } => cmd_tick(&track, &slug, true),
        Cmd::Untick { track, slug } => cmd_tick(&track, &slug, false),
        Cmd::Block {
            track,
            slug,
            question,
        } => cmd_block(&track, &slug, &question),
        Cmd::Unblock { track, slug } => cmd_unblock(&track, &slug),
        Cmd::Rename {
            track,
            old_slug,
            new_slug,
        } => cmd_rename(&track, &old_slug, &new_slug),
        Cmd::Close { track } => cmd_close(&track),
        Cmd::Decisions { track } => cmd_decisions(&track),
        Cmd::Claim {
            track,
            slug,
            max_turns,
        } => cmd_claim(&track, &slug, max_turns),
        Cmd::Release => cmd_release(),
        Cmd::ClaimStatus => cmd_claim_status(),
        Cmd::InitHook => cmd_init_hook(),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Stats { track } => cmd_stats(track.as_deref()),
        Cmd::Ask {
            track,
            slug,
            question,
            rule,
            context,
        } => cmd_ask(&track, &slug, &question, rule.as_deref(), context.as_deref()),
        Cmd::Questions { track } => cmd_questions(track.as_deref()),
        Cmd::Answer {
            track,
            slug,
            index,
            answer,
        } => cmd_answer(&track, &slug, index, &answer),
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
        if dir.join("driver").join("tracks.md").exists() {
            return Ok(dir.join("driver"));
        }
        if !dir.pop() {
            return Err("no driver/tracks.md found in this directory or any parent".into());
        }
    }
}

// ---- tracks.md ----

#[derive(Debug, Clone)]
struct TrackEntry {
    done: bool,
    id: String,
    #[allow(dead_code)]
    summary: String,
}

fn read_tracks() -> Result<(PathBuf, Vec<TrackEntry>), String> {
    let root = find_driver_root()?;
    let tracks_md = root.join("tracks.md");
    let text = fs::read_to_string(&tracks_md).map_err(|e| format!("read tracks.md: {e}"))?;
    let mut tracks = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let done = t.starts_with("- [x]");
        if !done && !t.starts_with("- [ ]") {
            continue;
        }
        let rest = t[5..].trim_start();
        let Some(s) = rest.find('[') else { continue };
        let Some(e) = rest.find(']') else { continue };
        if e <= s + 1 {
            continue;
        }
        let id = rest[s + 1..e].to_string();
        let summary = rest[e..]
            .split_once('—')
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_default();
        tracks.push(TrackEntry { done, id, summary });
    }
    Ok((root, tracks))
}

// ---- plan.md ----

#[derive(Debug, Clone)]
struct Task {
    done: bool,
    slug: String,
    #[allow(dead_code)]
    estimate: Option<u32>,
    depends: Vec<String>,
    /// Line index of the task's first line in the source.
    line: usize,
    description: String,
}

fn read_plan(root: &Path, track_id: &str) -> Result<(PathBuf, Vec<String>, Vec<Task>), String> {
    let plan_md = root.join("tracks").join(track_id).join("plan.md");
    if !plan_md.exists() {
        return Err(format!("no plan.md for track '{track_id}'"));
    }
    let text = fs::read_to_string(&plan_md).map_err(|e| format!("read plan.md: {e}"))?;
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let tasks = parse_tasks(&lines)?;
    Ok((plan_md, lines, tasks))
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

fn directly_blocked(root: &Path, track: &str, slug: &str) -> bool {
    root.join("tracks")
        .join(track)
        .join(format!("{slug}_blocked.md"))
        .exists()
}

fn transitive_blockers(
    root: &Path,
    track: &str,
    tasks: &[Task],
) -> HashMap<String, Vec<String>> {
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
            if s != t.slug && directly_blocked(root, track, s) {
                blockers.push(s.to_string());
            }
            if let Some(task) = by_slug.get(s) {
                for d in &task.depends {
                    queue.push_back(d.as_str());
                }
            }
        }
        // Also: a task is "blocked" if it's directly blocked itself.
        if directly_blocked(root, track, &t.slug) {
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

fn cmd_status() -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    if tracks.is_empty() {
        println!("No tracks. Use /driver:new to create one.");
        return Ok(());
    }
    println!("Driver status — {}", root.display());
    println!();
    let id_w = tracks.iter().map(|t| t.id.len()).max().unwrap_or(20);
    for t in &tracks {
        if t.done {
            println!("done    {:<id_w$}", t.id);
            continue;
        }
        match read_plan(&root, &t.id) {
            Ok((_, _, tasks)) => {
                let total = tasks.len();
                let complete = tasks.iter().filter(|x| x.done).count();
                let blockers = transitive_blockers(&root, &t.id, &tasks);
                let runnable: Vec<&Task> = tasks
                    .iter()
                    .filter(|x| is_runnable(x, &tasks, &blockers))
                    .collect();
                let blocked_count = tasks
                    .iter()
                    .filter(|x| !x.done && blockers.get(&x.slug).is_some_and(|b| !b.is_empty()))
                    .count();
                let marker = if !runnable.is_empty() {
                    "open"
                } else if blocked_count > 0 {
                    "BLOCK"
                } else {
                    "done?"
                };
                let next = runnable.first().map(|r| r.slug.clone());
                let next_str = match next {
                    Some(s) => format!("next: {s}"),
                    None if blocked_count > 0 => {
                        format!("all remaining tasks blocked ({blocked_count})")
                    }
                    None => "all tasks done — run `driver close`".to_string(),
                };
                println!(
                    "{marker:5}   {:<id_w$}  {complete}/{total} done — {next_str}",
                    t.id
                );
            }
            Err(e) => println!("open    {:<id_w$}  (plan error: {e})", t.id),
        }
    }
    Ok(())
}

fn cmd_next(track: Option<String>, json: bool) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let id = resolve_track(&root, &tracks, track.as_deref())?;
    let (_, _, tasks) = read_plan(&root, &id)?;
    let blockers = transitive_blockers(&root, &id, &tasks);
    let next = tasks
        .iter()
        .find(|t| is_runnable(t, &tasks, &blockers))
        .ok_or_else(|| format!("track '{id}' has no runnable tasks"))?;
    if json {
        print!("{}", task_json(&id, next, false));
    } else {
        print_task(next);
    }
    Ok(())
}

fn cmd_runnable(track: Option<String>, json: bool) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let id = resolve_track(&root, &tracks, track.as_deref())?;
    let (_, _, tasks) = read_plan(&root, &id)?;
    let blockers = transitive_blockers(&root, &id, &tasks);
    let runnable: Vec<&Task> = tasks
        .iter()
        .filter(|t| is_runnable(t, &tasks, &blockers))
        .collect();
    if json {
        print!("{{\"track_id\": {}, \"runnable\": [\n", json_str(&id));
        for (i, t) in runnable.iter().enumerate() {
            let sep = if i + 1 == runnable.len() { "" } else { "," };
            print!("  {}{sep}\n", task_json_compact(t));
        }
        print!("]}}\n");
    } else {
        if runnable.is_empty() {
            println!("No runnable tasks in '{id}'.");
        }
        for t in &runnable {
            print_task(t);
        }
    }
    Ok(())
}

fn cmd_tasks(track: Option<String>) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let id = resolve_track(&root, &tracks, track.as_deref())?;
    let (_, _, tasks) = read_plan(&root, &id)?;
    let blockers = transitive_blockers(&root, &id, &tasks);
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

fn cmd_blocked(track: Option<String>) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let id = resolve_track(&root, &tracks, track.as_deref())?;
    let (_, _, tasks) = read_plan(&root, &id)?;
    let mut any = false;
    for t in &tasks {
        if directly_blocked(&root, &id, &t.slug) {
            any = true;
            let path = root
                .join("tracks")
                .join(&id)
                .join(format!("{}_blocked.md", t.slug));
            let q = fs::read_to_string(&path).unwrap_or_default();
            println!("[{}] {}", t.slug, q.trim());
        }
    }
    if !any {
        println!("No blocked tasks in '{id}'.");
    }
    Ok(())
}

fn cmd_tick(track: &str, slug: &str, done: bool) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let (path, mut lines, tasks) = read_plan(&root, track)?;
    let task = tasks
        .iter()
        .find(|t| t.slug == slug)
        .ok_or_else(|| format!("no task '{slug}' in '{track}'"))?;
    let estimate = task.estimate;
    // Enforce principles + answered-status when ticking to done.
    if done {
        let active = read_active(&root);
        let start_commit = active
            .as_ref()
            .filter(|c| c.track == track && c.slug == slug)
            .map(|c| c.start_commit.clone());
        enforce_tick_gates(&root, track, slug, start_commit.as_deref())?;
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
        // If ticking to done and there's a matching active claim, record
        // estimate-vs-actual to history and release the claim. This
        // closes the loop without needing the post-tick gate to fire.
        if done {
            if let Some(claim) = read_active(&root) {
                if claim.track == track && claim.slug == slug {
                    let actual = claim.turn;
                    append_history(
                        &root,
                        &HistoryRecord {
                            ts: iso_now(),
                            track: track.to_string(),
                            slug: slug.to_string(),
                            estimate,
                            actual_turns: actual,
                            status: "done",
                        },
                    );
                    delete_active(&root);
                    if let Some(e) = estimate {
                        println!(
                            "  budget: {actual}/{} turns (est {e}, ratio {:.2})",
                            claim.max_turns,
                            actual as f64 / e as f64
                        );
                    } else {
                        println!("  budget: {actual}/{} turns (no estimate)", claim.max_turns);
                    }
                }
            }
        }
    } else {
        println!("No change ({slug} already {}).", if done { "done" } else { "open" });
    }
    Ok(())
}

fn cmd_block(track: &str, slug: &str, question: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let (_, _, tasks) = read_plan(&root, track)?;
    if !tasks.iter().any(|t| t.slug == slug) {
        return Err(format!("no task '{slug}' in '{track}'"));
    }
    let path = root
        .join("tracks")
        .join(track)
        .join(format!("{slug}_blocked.md"));
    fs::write(&path, format!("{}\n", question.trim_end())).map_err(|e| format!("write: {e}"))?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn cmd_unblock(track: &str, slug: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = root
        .join("tracks")
        .join(track)
        .join(format!("{slug}_blocked.md"));
    if !path.exists() {
        println!("{slug} is not blocked.");
        return Ok(());
    }
    fs::remove_file(&path).map_err(|e| format!("remove: {e}"))?;
    println!("Removed {}", path.display());
    Ok(())
}

fn cmd_rename(track: &str, old: &str, new: &str) -> Result<(), String> {
    if old == new {
        return Err("old and new slug are identical".into());
    }
    let (root, _) = read_tracks()?;
    let (path, lines, tasks) = read_plan(&root, track)?;
    if !tasks.iter().any(|t| t.slug == old) {
        return Err(format!("no task '{old}' in '{track}'"));
    }
    if tasks.iter().any(|t| t.slug == new) {
        return Err(format!("task '{new}' already exists in '{track}'"));
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
    // Rename associated files.
    let track_dir = root.join("tracks").join(track);
    for suffix in ["_design.md", "_blocked.md"] {
        let from = track_dir.join(format!("{old}{suffix}"));
        if from.exists() {
            let to = track_dir.join(format!("{new}{suffix}"));
            fs::rename(&from, &to).map_err(|e| format!("rename {}: {e}", from.display()))?;
            println!("Renamed {} → {}", from.display(), to.display());
        }
    }
    println!("Renamed task {old} → {new} in {track}.");
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

fn cmd_close(track: &str) -> Result<(), String> {
    let (root, mut tracks) = read_tracks()?;
    let (_, _, tasks) = read_plan(&root, track)?;
    let unfinished: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();
    if !unfinished.is_empty() {
        let mut msg = format!("track '{track}' has open tasks:\n");
        for t in unfinished {
            msg.push_str(&format!("  - {}\n", t.slug));
        }
        return Err(msg.trim_end().to_string());
    }
    let entry = tracks
        .iter_mut()
        .find(|t| t.id == track)
        .ok_or_else(|| format!("track '{track}' not in tracks.md"))?;
    if entry.done {
        println!("Already closed.");
        return Ok(());
    }
    let tracks_md = root.join("tracks.md");
    let text = fs::read_to_string(&tracks_md).map_err(|e| format!("read: {e}"))?;
    let new_text = text
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with("- [ ]") && line.contains(&format!("[{track}]")) {
                line.replacen("- [ ]", "- [x]", 1)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let new_text = if text.ends_with('\n') {
        format!("{new_text}\n")
    } else {
        new_text
    };
    fs::write(&tracks_md, new_text).map_err(|e| format!("write: {e}"))?;
    println!("Closed {track}.");
    Ok(())
}

fn cmd_decisions(track: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = root.join("tracks").join(track).join("decisions.md");
    if !path.exists() {
        println!("No decisions logged for {track}.");
        return Ok(());
    }
    print!(
        "{}",
        fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?
    );
    Ok(())
}

// ---- helpers ----

fn resolve_track(root: &Path, tracks: &[TrackEntry], arg: Option<&str>) -> Result<String, String> {
    if let Some(id) = arg {
        if tracks.iter().any(|t| t.id == id) {
            return Ok(id.to_string());
        }
        return Err(format!("no such track: {id}"));
    }
    let open: Vec<&TrackEntry> = tracks.iter().filter(|t| !t.done).collect();
    if open.is_empty() {
        return Err("no open tracks".into());
    }
    if open.len() == 1 {
        return Ok(open[0].id.clone());
    }
    open.iter()
        .filter_map(|t| {
            let mtime = fs::metadata(root.join("tracks").join(&t.id).join("plan.md"))
                .and_then(|m| m.modified())
                .ok()?;
            Some((mtime, t.id.clone()))
        })
        .max_by_key(|(m, _)| *m)
        .map(|(_, id)| id)
        .ok_or_else(|| "could not stat any open track's plan.md".into())
}

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

fn task_json(track: &str, t: &Task, _verbose: bool) -> String {
    let mut s = String::from("{\n");
    s.push_str(&format!("  \"track_id\": {},\n", json_str(track)));
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

fn task_json_compact(t: &Task) -> String {
    format!(
        "{{\"slug\": {}, \"estimate\": {}, \"depends\": [{}], \"description\": {}}}",
        json_str(&t.slug),
        t.estimate
            .map(|e| e.to_string())
            .unwrap_or("null".into()),
        t.depends
            .iter()
            .map(|d| json_str(d))
            .collect::<Vec<_>>()
            .join(", "),
        json_str(&t.description)
    )
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
// hook (configured via `driver init-hook`) runs `driver gate` after
// every agent turn. The gate increments the turn counter, releases the
// claim if the task has been ticked or blocked or budget exhausted, and
// otherwise exits 2 to prevent the agent from stopping.

#[derive(Debug, Clone)]
struct ActiveClaim {
    track: String,
    slug: String,
    max_turns: u32,
    turn: u32,
    started_at: String,
    /// Git HEAD commit at claim time, or empty if not in a git repo. Used by
    /// `driver tick` to diff what changed during the task against the
    /// principles.md rules.
    start_commit: String,
}

fn active_path(root: &Path) -> PathBuf {
    root.join(".active")
}

fn read_active(root: &Path) -> Option<ActiveClaim> {
    let path = active_path(root);
    let text = fs::read_to_string(&path).ok()?;
    let mut track = String::new();
    let mut slug = String::new();
    let mut max_turns: u32 = 50;
    let mut turn: u32 = 0;
    let mut started_at = String::new();
    let mut start_commit = String::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "track" => track = v.trim().to_string(),
            "slug" => slug = v.trim().to_string(),
            "max_turns" => max_turns = v.trim().parse().unwrap_or(50),
            "turn" => turn = v.trim().parse().unwrap_or(0),
            "started_at" => started_at = v.trim().to_string(),
            "start_commit" => start_commit = v.trim().to_string(),
            _ => {}
        }
    }
    if track.is_empty() || slug.is_empty() {
        return None;
    }
    Some(ActiveClaim {
        track,
        slug,
        max_turns,
        turn,
        started_at,
        start_commit,
    })
}

fn write_active(root: &Path, claim: &ActiveClaim) -> Result<(), String> {
    let path = active_path(root);
    let body = format!(
        "track={}\nslug={}\nmax_turns={}\nturn={}\nstarted_at={}\nstart_commit={}\n",
        claim.track,
        claim.slug,
        claim.max_turns,
        claim.turn,
        claim.started_at,
        claim.start_commit,
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

fn cmd_claim(track: &str, slug: &str, max_turns: u32) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let (_, _, tasks) = read_plan(&root, track)?;
    let task = tasks
        .iter()
        .find(|t| t.slug == slug)
        .ok_or_else(|| format!("no task '{slug}' in '{track}'"))?;
    if task.done {
        return Err(format!("task '{slug}' is already done"));
    }
    if let Some(existing) = read_active(&root) {
        return Err(format!(
            "claim already active for {}/{} (turn {}/{}). Release first with `driver release` if you want to switch.",
            existing.track, existing.slug, existing.turn, existing.max_turns
        ));
    }
    let start_commit = git_head_commit(&root).unwrap_or_default();
    let claim = ActiveClaim {
        track: track.to_string(),
        slug: slug.to_string(),
        max_turns,
        turn: 0,
        started_at: iso_now(),
        start_commit,
    };
    write_active(&root, &claim)?;
    println!(
        "Claimed {}/{} for up to {max_turns} turns.",
        claim.track, claim.slug
    );
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
        Some(c) => println!("Released {}/{}.", c.track, c.slug),
        None => println!("No active claim."),
    }
    Ok(())
}

fn cmd_claim_status() -> Result<(), String> {
    let root = find_driver_root()?;
    match read_active(&root) {
        Some(c) => {
            println!(
                "Active: {}/{} — turn {}/{}, started {}",
                c.track, c.slug, c.turn, c.max_turns, c.started_at
            );
            Ok(())
        }
        None => {
            println!("No active claim.");
            Ok(())
        }
    }
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
    if let Ok((_, _, tasks)) = read_plan(&root, &claim.track) {
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

    // Check whether the task was blocked.
    let blocked_path = root
        .join("tracks")
        .join(&claim.track)
        .join(format!("{}_blocked.md", claim.slug));
    if blocked_path.exists() {
        delete_active(&root);
        return ExitCode::SUCCESS;
    }

    // Budget check.
    if claim.turn > claim.max_turns {
        delete_active(&root);
        eprintln!(
            "driver gate: turn budget exceeded for {}/{} ({}/{}). Released claim — agent may stop.",
            claim.track, claim.slug, claim.turn, claim.max_turns
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
        "[driver] turn {turn}/{max} — {track}/{slug}{warn}. Tick when done or `driver block {track} {slug} \"<question>\"`.",
        slug = claim.slug,
        track = claim.track,
        turn = claim.turn,
        max = claim.max_turns,
    );
    ExitCode::from(2)
}

// ---- history (.history.jsonl) + stats ----

struct HistoryRecord {
    ts: String,
    track: String,
    slug: String,
    estimate: Option<u32>,
    actual_turns: u32,
    status: &'static str,
}

fn history_path(root: &Path) -> PathBuf {
    root.join(".history.jsonl")
}

fn append_history(root: &Path, rec: &HistoryRecord) {
    use std::io::Write as _;
    let path = history_path(root);
    let estimate_s = rec
        .estimate
        .map(|e| e.to_string())
        .unwrap_or_else(|| "null".into());
    let line = format!(
        "{{\"ts\": {}, \"track\": {}, \"slug\": {}, \"estimate\": {estimate_s}, \"actual_turns\": {}, \"status\": {}}}\n",
        json_str(&rec.ts),
        json_str(&rec.track),
        json_str(&rec.slug),
        rec.actual_turns,
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
        let track = json_field(line, "track").unwrap_or_default();
        let slug = json_field(line, "slug").unwrap_or_default();
        let estimate = json_number(line, "estimate");
        let actual = json_number(line, "actual_turns").unwrap_or(0);
        let status_owned = json_field(line, "status").unwrap_or_else(|| "done".into());
        let status: &'static str = match status_owned.as_str() {
            "blocked" => "blocked",
            _ => "done",
        };
        out.push(HistoryRecord {
            ts,
            track,
            slug,
            estimate,
            actual_turns: actual,
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

fn cmd_stats(track_filter: Option<&str>) -> Result<(), String> {
    let root = find_driver_root()?;
    let records = read_history(&root);
    let mut records: Vec<&HistoryRecord> = records
        .iter()
        .filter(|r| r.status == "done")
        .filter(|r| track_filter.map(|t| t == r.track).unwrap_or(true))
        .collect();
    if records.is_empty() {
        if let Some(t) = track_filter {
            println!("No history for track '{t}'.");
        } else {
            println!("No history yet. Tick a claimed task to populate .history.jsonl.");
        }
        return Ok(());
    }
    records.sort_by_key(|r| r.ts.clone());

    let n = records.len();
    let actuals: Vec<u32> = records.iter().map(|r| r.actual_turns).collect();
    let total: u32 = actuals.iter().sum();
    let mean = total as f64 / n as f64;
    let mut sorted = actuals.clone();
    sorted.sort();
    let median = if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
    };

    let with_est: Vec<(&HistoryRecord, u32)> = records
        .iter()
        .filter_map(|r| r.estimate.map(|e| (*r, e)))
        .collect();
    let est_ratio_mean: Option<f64> = if with_est.is_empty() {
        None
    } else {
        let sum: f64 = with_est
            .iter()
            .map(|(r, e)| r.actual_turns as f64 / *e as f64)
            .sum();
        Some(sum / with_est.len() as f64)
    };

    println!("Driver stats — {n} completed task(s){}", track_filter.map(|t| format!(" in {t}")).unwrap_or_default());
    println!("  total turns:   {total}");
    println!("  mean turns:    {mean:.1}");
    println!("  median turns:  {median:.1}");
    if let Some(r) = est_ratio_mean {
        println!(
            "  actual/est:    mean {r:.2} ({})",
            if r > 1.10 {
                "estimates run light"
            } else if r < 0.90 {
                "estimates run heavy"
            } else {
                "well-calibrated"
            }
        );
    }

    // Top 3 over/under, by absolute ratio.
    if !with_est.is_empty() {
        let mut by_ratio: Vec<(&HistoryRecord, u32, f64)> = with_est
            .iter()
            .map(|(r, e)| (*r, *e, r.actual_turns as f64 / *e as f64))
            .collect();
        by_ratio.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        let under: Vec<_> = by_ratio.iter().take(3).collect();
        let over: Vec<_> = by_ratio.iter().rev().take(3).collect();
        if !under.is_empty() {
            println!("\nLargest under-estimates (actual ≫ est):");
            for (r, e, ratio) in under {
                println!(
                    "  {slug:<28} ~{e}t est, {a}t actual ({ratio:.2}×) [{track}]",
                    slug = r.slug,
                    a = r.actual_turns,
                    track = r.track,
                );
            }
        }
        if !over.is_empty() {
            println!("\nLargest over-estimates (actual ≪ est):");
            for (r, e, ratio) in over {
                println!(
                    "  {slug:<28} ~{e}t est, {a}t actual ({ratio:.2}×) [{track}]",
                    slug = r.slug,
                    a = r.actual_turns,
                    track = r.track,
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
    let (hook_ok, hook_detail) = check_stop_hook(&bin);
    report(hook_ok, "Stop hook installed", &hook_detail);
    all_ok &= hook_ok;

    // 3. cwd inside a driver project? Informational — doctor passes
    // for global setup even without a project. Mark with ℹ when absent.
    match find_driver_root() {
        Ok(root) => {
            report(true, "driver/ project in cwd", &root.display().to_string());
            if let Some(c) = read_active(&root) {
                report(
                    true,
                    "Active claim",
                    &format!("{}/{} — turn {}/{}", c.track, c.slug, c.turn, c.max_turns),
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

fn check_stop_hook(bin: &str) -> (bool, String) {
    let Some(home) = std::env::var_os("HOME") else {
        return (false, "$HOME not set".into());
    };
    let path = PathBuf::from(home).join(".claude").join("settings.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return (
            false,
            format!("{} not found. Run `driver init-hook` and paste the snippet.", path.display()),
        );
    };
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
            format!(
                "{} has no Stop hook entry for driver. Run `driver init-hook`.",
                path.display()
            ),
        )
    }
}

fn report(ok: bool, label: &str, detail: &str) {
    let mark = if ok { "✓" } else { "✗" };
    println!("{mark} {label:<28} {detail}");
}

fn cmd_init_hook() -> Result<(), String> {
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "driver".to_string());
    let snippet = format!(
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
    );
    println!("Paste this into ~/.claude/settings.json (or .claude/settings.json for project-local):");
    println!();
    println!("{snippet}");
    println!();
    println!("If you already have a hooks section, merge the Stop array entry into it.");
    println!("Verify with: driver gate ; echo $? (should print 0 when no claim is active)");
    Ok(())
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

fn questions_path(root: &Path, track: &str, slug: &str) -> PathBuf {
    root.join("tracks").join(track).join(format!("{slug}_questions.md"))
}

#[derive(Debug, Clone)]
struct Question {
    /// 1-based index within the file.
    index: u32,
    summary: String,
    rule: Option<String>,
    context: String,
    answered: bool,
}

fn read_questions(root: &Path, track: &str, slug: &str) -> Vec<Question> {
    let path = questions_path(root, track, slug);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<Question> = Vec::new();
    let mut cur_summary: Option<String> = None;
    let mut cur_rule: Option<String> = None;
    let mut cur_context = String::new();
    let mut cur_answered = false;
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
                });
            }
            cur_answered = false;
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
        });
    }
    out
}

fn cmd_ask(
    track: &str,
    slug: &str,
    question: &str,
    rule: Option<&str>,
    context: Option<&str>,
) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let (_, _, tasks) = read_plan(&root, track)?;
    if !tasks.iter().any(|t| t.slug == slug) {
        return Err(format!("no task '{slug}' in '{track}'"));
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
    let existing = read_questions(&root, track, slug);
    let next_idx = existing.iter().map(|q| q.index).max().unwrap_or(0) + 1;
    let path = questions_path(&root, track, slug);
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
    if let Some(c) = context {
        body.push_str(&format!("**context:** {}\n", c.trim()));
    }
    body.push_str("**answer:** _pending_\n");
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    println!(
        "Q{next_idx} logged for {track}/{slug}{} → {}",
        rule.map(|r| format!(" [rule={r}]")).unwrap_or_default(),
        path.display()
    );
    Ok(())
}

/// Record an answer to question <index> in <slug>_questions.md.
/// Walks the file line-by-line, finds the matching `## Q<index>:`
/// section, and replaces its `**answer:** ...` line (or the first
/// `**answer:**` line within that section). Idempotent — running
/// twice replaces the prior answer.
fn cmd_answer(track: &str, slug: &str, index: u32, answer: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = questions_path(&root, track, slug);
    if !path.exists() {
        return Err(format!("no questions file for {track}/{slug}"));
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
    println!("Q{index} answered for {track}/{slug}.");
    Ok(())
}

fn cmd_questions(track_filter: Option<&str>) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let target_tracks: Vec<String> = match track_filter {
        Some(t) => {
            if !tracks.iter().any(|x| x.id == t) {
                return Err(format!("no such track: {t}"));
            }
            vec![t.to_string()]
        }
        None => tracks.iter().filter(|x| !x.done).map(|x| x.id.clone()).collect(),
    };
    let mut total = 0u32;
    let mut total_unanswered = 0u32;
    for track_id in &target_tracks {
        let Ok((_, _, tasks)) = read_plan(&root, track_id) else {
            continue;
        };
        for task in &tasks {
            let qs = read_questions(&root, track_id, &task.slug);
            if qs.is_empty() {
                continue;
            }
            println!("[{track_id}/{}]", task.slug);
            for q in &qs {
                total += 1;
                let mark = if q.answered { "✓" } else { "·" };
                if !q.answered {
                    total_unanswered += 1;
                }
                let rule_tag = q
                    .rule
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default();
                println!("  {mark} Q{}{rule_tag}: {}", q.index, q.summary);
                if !q.context.is_empty() {
                    for line in textwrap_simple(&q.context, 72) {
                        println!("      {line}");
                    }
                }
            }
        }
    }
    if total == 0 {
        println!("No open questions.");
    } else {
        println!(
            "\n{total} question(s) total, {total_unanswered} unanswered."
        );
    }
    Ok(())
}

/// Check the principles floor + answered-status for a task that's
/// about to be ticked. Returns Err with a diagnostic if anything fails.
fn enforce_tick_gates(
    root: &Path,
    track: &str,
    slug: &str,
    start_commit: Option<&str>,
) -> Result<(), String> {
    // Floor: triggered files touched since claim start must have a
    // matching rule-tagged question.
    let rules = read_principles(root);
    let questions = read_questions(root, track, slug);
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
                        "principles rule '{}' tripped — touched: {}\n  → run `driver ask {track} {slug} --rule {} \"<question>\" --context \"...\"` first, or revert the change.",
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
