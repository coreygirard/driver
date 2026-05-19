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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
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
