use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use tokio::sync::Mutex;

use steno_core::db::Database;
use steno_core::model::StoredEvent;

pub async fn run(db: Arc<Mutex<Database>>, interval_secs: u64) {
    // Initial delay to let events accumulate
    tokio::time::sleep(Duration::from_secs(60)).await;

    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        ticker.tick().await;
        let db = db.lock().await;
        if let Err(e) = process_ungrouped(&db) {
            tracing::warn!("Grouper error: {}", e);
        }
    }
}

fn process_ungrouped(db: &Database) -> steno_core::Result<()> {
    let events = db.get_ungrouped_events(1000)?;
    if events.is_empty() {
        return Ok(());
    }

    let groups = build_groups(&events);
    let mut total_grouped = 0u32;

    for group in groups {
        if group.events.len() < 2 {
            continue;
        }

        let date = group.events[0].started_at.get(..10).unwrap_or("unknown");
        let base_name = format!("{}-{}", group.category, date);

        let name = find_unique_name(db, &base_name)?;
        let description = format!(
            "Auto-grouped {} {} commands",
            group.events.len(),
            group.category
        );

        let commands: Vec<String> = group.events.iter().map(|e| e.command.clone()).collect();
        let confidence = 1.0 / (group.events.len() as f64);

        let task_id = db.upsert_auto_task(&name, Some(&description), &commands, confidence)?;

        let event_ids: Vec<i64> = group.events.iter().map(|e| e.id).collect();
        let count = db.attach_task_to_events(task_id, &event_ids)?;
        total_grouped += count;
    }

    if total_grouped > 0 {
        tracing::info!("Grouper assigned {} events to auto-tasks", total_grouped);
    }

    Ok(())
}

struct EventGroup<'a> {
    events: Vec<&'a StoredEvent>,
    category: String,
}

fn build_groups(events: &[StoredEvent]) -> Vec<EventGroup<'_>> {
    let mut groups: Vec<EventGroup<'_>> = Vec::new();
    let mut current_events: Vec<&StoredEvent> = Vec::new();
    let mut current_category = String::new();

    for event in events {
        if current_events.is_empty() {
            current_category = classify_command(&event.command);
            current_events.push(event);
            continue;
        }

        let prev = current_events.last().unwrap();
        if should_group(prev, event) {
            let cat = classify_command(&event.command);
            if cat != "other" {
                current_category = cat;
            }
            current_events.push(event);
        } else {
            groups.push(EventGroup {
                events: std::mem::take(&mut current_events),
                category: std::mem::take(&mut current_category),
            });
            current_category = classify_command(&event.command);
            current_events.push(event);
        }
    }

    if !current_events.is_empty() {
        groups.push(EventGroup {
            events: current_events,
            category: current_category,
        });
    }

    groups
}

fn should_group(a: &StoredEvent, b: &StoredEvent) -> bool {
    if a.session_id != b.session_id {
        return false;
    }

    let time_close = if let (Ok(ta), Ok(tb)) = (
        DateTime::parse_from_rfc3339(&a.started_at),
        DateTime::parse_from_rfc3339(&b.started_at),
    ) {
        (tb - ta).num_seconds().unsigned_abs() <= 60
    } else {
        false
    };

    if !time_close {
        return false;
    }

    let same_location = a.cwd == b.cwd || (a.git_root.is_some() && a.git_root == b.git_root);
    let same_category = classify_command(&a.command) == classify_command(&b.command);

    same_location || same_category
}

fn classify_command(cmd: &str) -> String {
    let first_word = cmd.split_whitespace().next().unwrap_or("");

    let effective = if first_word == "sudo" || first_word == "env" {
        cmd.split_whitespace().nth(1).unwrap_or(first_word)
    } else {
        first_word
    };

    match effective {
        "git" | "gh" | "hub" => "git",
        "cargo" | "rustc" | "rustup" | "clippy" => "rust-build",
        "npm" | "npx" | "yarn" | "pnpm" | "bun" | "node" | "deno" | "tsc" => "node-build",
        "make" | "cmake" | "ninja" | "meson" | "gcc" | "g++" | "cc" | "clang" => "build",
        "docker" | "podman" | "docker-compose" | "kubectl" | "helm" => "container",
        "cd" | "pushd" | "popd" | "ls" | "ll" | "la" | "tree" | "find" => "navigate",
        _ => "other",
    }
    .to_string()
}

fn find_unique_name(db: &Database, base: &str) -> steno_core::Result<String> {
    if db.get_task_by_name(base).is_err() {
        return Ok(base.to_string());
    }

    for i in 2..=100 {
        let candidate = format!("{}-{}", base, i);
        if db.get_task_by_name(&candidate).is_err() {
            return Ok(candidate);
        }
    }

    Ok(format!(
        "{}-{}",
        base,
        &uuid::Uuid::new_v4().to_string()[..8]
    ))
}
