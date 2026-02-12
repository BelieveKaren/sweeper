use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ProjectItem {
    pub path: PathBuf,
    pub last_modified: SystemTime,
}

#[derive(Debug, Clone)]
pub struct ScanReport {
    pub root: PathBuf,
    pub older_than_days: u64,
    pub stale: Vec<ProjectItem>,
    pub fresh: Vec<ProjectItem>,
    pub scanned_count: usize,
}

#[derive(Debug, Clone)]
pub struct ArchiveMove {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ArchivePlan {
    pub dest_root: PathBuf,
    pub month_bucket: String, // e.g. "2026-02"
    pub moves: Vec<ArchiveMove>,
}

pub fn scan_projects(root: &Path, older_than_days: u64) -> Result<ScanReport> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Cannot access path: {}", root.display()))?;

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(older_than_days * 24 * 60 * 60))
        .context("Failed to compute cutoff time")?;

    // Only scan immediate subdirectories (projects), not recursive by default.
    let mut stale = Vec::new();
    let mut fresh = Vec::new();
    let mut scanned = 0;

    for entry in fs::read_dir(&root).with_context(|| format!("read_dir failed: {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        // Skip non-directories
        if !path.is_dir() {
            continue;
        }

        // Optional: skip hidden dirs
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }

        scanned += 1;

        // Determine "last modified" of the folder by looking at the newest file inside it.
        let last_modified = newest_mtime_in_tree(&path).unwrap_or_else(|| {
            // fallback: folder metadata mtime
            fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });

        let item = ProjectItem { path, last_modified };

        if last_modified <= cutoff {
            stale.push(item);
        } else {
            fresh.push(item);
        }
    }

    // sort stale oldest first for nicer output
    stale.sort_by_key(|p| p.last_modified);

    Ok(ScanReport {
        root,
        older_than_days,
        stale,
        fresh,
        scanned_count: scanned,
    })
}

fn newest_mtime_in_tree(dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;

    for e in WalkDir::new(dir)
        .max_depth(3) // keep it fast; change to higher if you want
        .into_iter()
        .filter_map(|x| x.ok())
    {
        if let Ok(meta) = e.metadata() {
            if let Ok(mtime) = meta.modified() {
                newest = match newest {
                    None => Some(mtime),
                    Some(cur) => Some(cur.max(mtime)),
                };
            }
        }
    }

    newest
}

pub fn build_archive_plan(report: &ScanReport, dest_root: &Path) -> Result<ArchivePlan> {
    let dest_root = dest_root
        .to_path_buf()
        .canonicalize()
        .unwrap_or_else(|_| dest_root.to_path_buf()); // allow non-existing path

    let now: DateTime<Local> = Local::now();
    let bucket = now.format("%Y-%m").to_string(); // e.g. 2026-02

    let bucket_dir = dest_root.join(&bucket);

    let mut moves = Vec::new();

    for item in &report.stale {
        // Avoid moving the archive folder into itself if user points root incorrectly.
        if item.path.starts_with(&dest_root) {
            continue;
        }

        let name = item
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let mut to = bucket_dir.join(&name);
        to = avoid_collision(&to);

        moves.push(ArchiveMove {
            from: item.path.clone(),
            to,
        });
    }

    Ok(ArchivePlan {
        dest_root,
        month_bucket: bucket,
        moves,
    })
}

pub fn apply_archive_plan(plan: &ArchivePlan) -> Result<()> {
    for mv in &plan.moves {
        if let Some(parent) = mv.to.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dir: {}", parent.display()))?;
        }

        // rename = move (on same filesystem). If different filesystem, you’d need copy+delete.
        fs::rename(&mv.from, &mv.to).with_context(|| {
            format!(
                "Failed to move '{}' -> '{}'",
                mv.from.display(),
                mv.to.display()
            )
        })?;
    }
    Ok(())
}

fn avoid_collision(target: &Path) -> PathBuf {
    if !target.exists() {
        return target.to_path_buf();
    }
    let mut i = 1;
    loop {
        let candidate = PathBuf::from(format!("{}_{}", target.display(), i));
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

pub fn print_report(report: &ScanReport) {
    println!("Root: {}", report.root.display());
    println!("Scanned project folders: {}", report.scanned_count);
    println!("Stale threshold: {} days\n", report.older_than_days);

    if report.stale.is_empty() {
        println!("No stale folders found. ✅");
        return;
    }

    println!("Stale folders (oldest first):");
    for (idx, item) in report.stale.iter().enumerate() {
        println!(
            "  {:>2}. {}  (last modified: {})",
            idx + 1,
            item.path.display(),
            fmt_time(item.last_modified)
        );
    }
}

pub fn print_plan(plan: &ArchivePlan) {
    println!("Archive destination: {}", plan.dest_root.display());
    println!("Month bucket: {}", plan.month_bucket);
    println!("Planned moves: {}\n", plan.moves.len());

    for (idx, mv) in plan.moves.iter().enumerate() {
        println!(
            "  {:>2}. '{}' -> '{}'",
            idx + 1,
            mv.from.display(),
            mv.to.display()
        );
    }
}

fn fmt_time(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub fn organize_folder(path: &std::path::Path, dry_run: bool) -> anyhow::Result<()> {
    use std::fs;

    let categories = |ext: &str| -> &str {
        match ext {
            "pdf" | "doc" | "docx" | "txt" => "Documents",
            "jpg" | "png" | "gif" | "webp" => "Images",
            "zip" | "rar" | "7z" | "tar" | "gz" => "Archives",
            "dmg" | "exe" | "msi" | "pkg" | "deb" | "rpm" => "Installers",
            "csv" | "xlsx" => "Spreadsheets",
            _ => "Other",
        }
    };

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_path = entry.path();

        if !file_path.is_file() {
            continue;
        }

        let ext = file_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let category = categories(&ext);
        let target_dir = path.join(category);

        let file_name = file_path.file_name().unwrap();
        let mut target_path = target_dir.join(file_name);

        // Avoid overwrite
        let mut counter = 1;
        while target_path.exists() {
            let new_name = format!(
                "{}_{}",
                file_name.to_string_lossy(),
                counter
            );
            target_path = target_dir.join(new_name);
            counter += 1;
        }

        println!("Move: '{}' -> '{}'", file_path.display(), target_path.display());

        if !dry_run {
            fs::create_dir_all(&target_dir)?;
            fs::rename(&file_path, &target_path)?;
        }
    }

    if dry_run {
        println!("\nDry-run only. Use without --dry-run to apply.");
    }

    Ok(())
}

pub fn delete_to_trash(items: &[ProjectItem]) -> anyhow::Result<()> {
    for item in items {
        trash::delete(&item.path)
            .with_context(|| format!("Failed to move '{}' to trash", item.path.display()))?;
    }
    Ok(())
}
