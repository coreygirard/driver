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
    })
}

fn write_active(root: &Path, claim: &ActiveClaim) -> Result<(), String> {
    let path = active_path(root);
    let body = format!(
        "track={}\nslug={}\nmax_turns={}\nturn={}\nstarted_at={}\n",
        claim.track, claim.slug, claim.max_turns, claim.turn, claim.started_at
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
    let claim = ActiveClaim {
        track: track.to_string(),
        slug: slug.to_string(),
        max_turns,
        turn: 0,
        started_at: iso_now(),
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

    // Block the stop.
    eprintln!(
        "Driver task '{slug}' (track {track}) is not done. Run `driver tick {track} {slug}` when complete, or `driver block {track} {slug} \"<question>\"` if you need design input. Turn {turn}/{max}.",
        slug = claim.slug,
        track = claim.track,
        turn = claim.turn,
        max = claim.max_turns,
    );
    ExitCode::from(2)
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
