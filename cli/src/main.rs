// driver — minimal track planner.
//
// Operates on files under driver/ in the current project. Discovers the
// project root by walking up from the cwd looking for driver/tracks.md.

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
    /// Show all tracks and the next unchecked phase per track.
    Status,
    /// Print the next unchecked phase of a track.
    Next {
        /// Track id. If omitted, picks the most recently modified open track.
        track: Option<String>,
        /// Output as JSON instead of human-readable.
        #[arg(long)]
        json: bool,
    },
    /// Tick all bullets of a phase as done.
    Tick {
        track: String,
        phase: usize,
    },
    /// Tick a single bullet within a phase.
    TickBullet {
        track: String,
        phase: usize,
        /// 1-indexed bullet position within the phase.
        bullet: usize,
    },
    /// Close a track if all phases are complete (flips registry to [x]).
    Close { track: String },
    /// Create blocked.md with a design question for the track.
    Block { track: String, question: String },
    /// Remove blocked.md for the track.
    Unblock { track: String },
    /// Print decisions.md for the track.
    Decisions { track: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Status => cmd_status(),
        Cmd::Next { track, json } => cmd_next(track, json),
        Cmd::Tick { track, phase } => cmd_tick(&track, phase, None),
        Cmd::TickBullet {
            track,
            phase,
            bullet,
        } => cmd_tick(&track, phase, Some(bullet)),
        Cmd::Close { track } => cmd_close(&track),
        Cmd::Block { track, question } => cmd_block(&track, &question),
        Cmd::Unblock { track } => cmd_unblock(&track),
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

// ---- project discovery ----

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

// ---- tracks.md parsing ----

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
        // "- [ ] [<id>](./tracks/<id>/plan.md) — <summary>"
        // "- [x] ..."
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- [") {
            continue;
        }
        let done = trimmed.starts_with("- [x]");
        if !done && !trimmed.starts_with("- [ ]") {
            continue;
        }
        // Extract id from [<id>](...)
        let rest = &trimmed[5..]; // after "- [ ]" or "- [x]"
        let rest = rest.trim_start();
        let id_start = rest.find('[').map(|i| i + 1);
        let id_end = rest.find(']');
        let (id, summary) = match (id_start, id_end) {
            (Some(s), Some(e)) if e > s => {
                let id = rest[s..e].to_string();
                // summary is whatever comes after " — "
                let after_link = &rest[e..];
                let summary = after_link
                    .split_once('—')
                    .map(|(_, s)| s.trim().to_string())
                    .unwrap_or_default();
                (id, summary)
            }
            _ => continue,
        };
        tracks.push(TrackEntry { done, id, summary });
    }
    Ok((root, tracks))
}

// ---- plan.md parsing ----

#[derive(Debug, Clone)]
struct Phase {
    /// 1-indexed phase number from the heading ("## Phase N: ...").
    number: usize,
    name: String,
    /// Optional turn estimate ("~10 turns" → 10).
    turn_estimate: Option<u32>,
    bullets: Vec<Bullet>,
}

#[derive(Debug, Clone)]
struct Bullet {
    done: bool,
    text: String,
    line: usize,
}

fn read_plan(root: &Path, track_id: &str) -> Result<(PathBuf, Vec<String>, Vec<Phase>), String> {
    let plan_md = root.join("tracks").join(track_id).join("plan.md");
    if !plan_md.exists() {
        return Err(format!("no plan.md for track '{track_id}'"));
    }
    let text = fs::read_to_string(&plan_md).map_err(|e| format!("read plan.md: {e}"))?;
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let phases = parse_phases(&lines);
    Ok((plan_md, lines, phases))
}

fn parse_phases(lines: &[String]) -> Vec<Phase> {
    let mut phases: Vec<Phase> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("## Phase ") {
            // "N: <name> (~K turns)" or "N: <name>"
            let (num_str, after_num) = rest.split_once(':').unwrap_or((rest, ""));
            let number: usize = num_str.trim().parse().unwrap_or(0);
            let name_and_est = after_num.trim();
            let (name, turn_estimate) = match name_and_est.rfind('(') {
                Some(idx) => {
                    let est_str = &name_and_est[idx + 1..];
                    let est = est_str
                        .trim_end_matches(')')
                        .trim_start_matches('~')
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u32>().ok());
                    (name_and_est[..idx].trim().to_string(), est)
                }
                None => (name_and_est.to_string(), None),
            };
            let _ = i;
            phases.push(Phase {
                number,
                name,
                turn_estimate,
                bullets: Vec::new(),
            });
        } else if let Some(last) = phases.last_mut() {
            let t = line.trim_start();
            if t.starts_with("- [ ]") || t.starts_with("- [x]") {
                let done = t.starts_with("- [x]");
                last.bullets.push(Bullet {
                    done,
                    text: t[5..].trim().to_string(),
                    line: i,
                });
            } else if !line.is_empty()
                && line.starts_with("  ")
                && !line.trim_start().starts_with('#')
            {
                // Continuation of the most recent bullet.
                if let Some(b) = last.bullets.last_mut() {
                    b.text.push(' ');
                    b.text.push_str(line.trim());
                }
            }
        }
    }
    phases
}

fn phase_complete(p: &Phase) -> bool {
    !p.bullets.is_empty() && p.bullets.iter().all(|b| b.done)
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
            println!("done  {:<id_w$}", t.id);
            continue;
        }
        let blocked = root
            .join("tracks")
            .join(&t.id)
            .join("blocked.md")
            .exists();
        let (root2, _) = (root.clone(), ());
        let _ = root2;
        let plan = read_plan(&root, &t.id);
        match plan {
            Ok((_, _, phases)) => {
                let total = phases.len();
                let complete = phases.iter().filter(|p| phase_complete(p)).count();
                let next = phases.iter().find(|p| !phase_complete(p));
                let next_str = match next {
                    Some(p) => {
                        let unchecked = p.bullets.iter().filter(|b| !b.done).count();
                        match p.turn_estimate {
                            Some(e) => format!(
                                "next: Phase {} ({}, ~{}t), {} unchecked",
                                p.number, p.name, e, unchecked
                            ),
                            None => format!(
                                "next: Phase {} ({}), {} unchecked",
                                p.number, p.name, unchecked
                            ),
                        }
                    }
                    None => "all phases complete — run `driver close`".to_string(),
                };
                let marker = if blocked { "BLOCKED" } else { "open " };
                println!(
                    "{marker} {:<id_w$}  phases {complete}/{total} — {next_str}",
                    t.id
                );
            }
            Err(e) => println!("open  {:<id_w$}  (plan error: {e})", t.id),
        }
    }
    Ok(())
}

fn cmd_next(track: Option<String>, json: bool) -> Result<(), String> {
    let (root, tracks) = read_tracks()?;
    let id = resolve_track(&root, &tracks, track.as_deref())?;
    let (_, _, phases) = read_plan(&root, &id)?;
    let phase = phases
        .iter()
        .find(|p| !phase_complete(p))
        .ok_or_else(|| format!("track '{id}' has no unchecked phases"))?;
    let unchecked: Vec<&Bullet> = phase.bullets.iter().filter(|b| !b.done).collect();
    let blocked = root.join("tracks").join(&id).join("blocked.md").exists();
    if json {
        let mut s = String::from("{\n");
        s.push_str(&format!("  \"track_id\": {},\n", json_str(&id)));
        s.push_str(&format!("  \"phase_number\": {},\n", phase.number));
        s.push_str(&format!("  \"phase_name\": {},\n", json_str(&phase.name)));
        if let Some(e) = phase.turn_estimate {
            s.push_str(&format!("  \"turn_estimate\": {e},\n"));
        }
        s.push_str(&format!("  \"blocked\": {blocked},\n"));
        s.push_str("  \"unchecked\": [\n");
        for (i, b) in unchecked.iter().enumerate() {
            let sep = if i + 1 == unchecked.len() { "" } else { "," };
            s.push_str(&format!("    {}{sep}\n", json_str(&b.text)));
        }
        s.push_str("  ]\n");
        s.push_str("}\n");
        print!("{s}");
    } else {
        if blocked {
            println!("track {id} is BLOCKED — see driver/tracks/{id}/blocked.md");
        }
        match phase.turn_estimate {
            Some(e) => println!("Phase {} ({}, ~{}t)", phase.number, phase.name, e),
            None => println!("Phase {} ({})", phase.number, phase.name),
        }
        for b in &unchecked {
            println!("  - {}", b.text);
        }
    }
    Ok(())
}

fn cmd_tick(track: &str, phase_num: usize, bullet: Option<usize>) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let (path, mut lines, phases) = read_plan(&root, track)?;
    let phase = phases
        .iter()
        .find(|p| p.number == phase_num)
        .ok_or_else(|| format!("no Phase {phase_num} in track '{track}'"))?;
    let targets: Vec<&Bullet> = match bullet {
        Some(idx) => {
            let b = phase
                .bullets
                .get(idx.checked_sub(1).ok_or("bullet index is 1-based")?)
                .ok_or_else(|| {
                    format!(
                        "bullet {idx} out of range (phase has {} bullets)",
                        phase.bullets.len()
                    )
                })?;
            vec![b]
        }
        None => phase.bullets.iter().collect(),
    };
    let mut changed = 0;
    for b in &targets {
        if !b.done {
            // Replace "- [ ]" with "- [x]" preserving leading whitespace.
            let l = &mut lines[b.line];
            if let Some(idx) = l.find("- [ ]") {
                l.replace_range(idx..idx + 5, "- [x]");
                changed += 1;
            }
        }
    }
    if changed == 0 {
        println!("No changes (already ticked).");
        return Ok(());
    }
    fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("write: {e}"))?;
    println!(
        "Ticked {changed} bullet{} in Phase {phase_num} of {track}.",
        if changed == 1 { "" } else { "s" }
    );
    Ok(())
}

fn cmd_close(track: &str) -> Result<(), String> {
    let (root, mut tracks) = read_tracks()?;
    let (_, _, phases) = read_plan(&root, track)?;
    let unfinished: Vec<&Phase> = phases.iter().filter(|p| !phase_complete(p)).collect();
    if !unfinished.is_empty() {
        let mut msg = format!("track '{track}' still has unchecked phases:\n");
        for p in unfinished {
            let n = p.bullets.iter().filter(|b| !b.done).count();
            msg.push_str(&format!("  Phase {} ({}): {n} unchecked\n", p.number, p.name));
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
    entry.done = true;
    // Rewrite tracks.md in place. Read original text, regex-flip just this one line.
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

fn cmd_block(track: &str, question: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = root.join("tracks").join(track).join("blocked.md");
    if !path.parent().is_some_and(|p| p.exists()) {
        return Err(format!("no such track: {track}"));
    }
    fs::write(&path, format!("{}\n", question.trim_end())).map_err(|e| format!("write: {e}"))?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn cmd_unblock(track: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = root.join("tracks").join(track).join("blocked.md");
    if !path.exists() {
        println!("Not blocked.");
        return Ok(());
    }
    fs::remove_file(&path).map_err(|e| format!("remove: {e}"))?;
    println!("Removed {}", path.display());
    Ok(())
}

fn cmd_decisions(track: &str) -> Result<(), String> {
    let (root, _) = read_tracks()?;
    let path = root.join("tracks").join(track).join("decisions.md");
    if !path.exists() {
        println!("No decisions logged for {track}.");
        return Ok(());
    }
    print!("{}", fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?);
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
    // Pick most recently modified plan.md among open tracks.
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
