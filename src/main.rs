use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::CompressionMethod;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const DEFAULT_FOLDER_NAME: &str = "backup_source";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    source_folder: Option<String>,
    #[serde(default)]
    source_folders: Vec<String>,
    destination_path: String,
    base_name: String,
    max_backups: usize,
    compression_level: Option<u8>,
}

impl Config {
    // Get all source folders, combining single and multiple configurations
    // If both source_folder and source_folders are set, source_folders takes precedence
    fn get_source_folders(&self) -> Vec<String> {
        if !self.source_folders.is_empty() {
            self.source_folders.clone()
        } else if let Some(ref folder) = self.source_folder {
            vec![folder.clone()]
        } else {
            vec![]
        }
    }
}

#[derive(Debug, Clone)]
struct BackupContext {
    config: Config,
    log_path: PathBuf,
    timestamp: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            source_folder: None,
            source_folders: vec![String::from("C:\\Users\\YourUsername\\Documents")],
            destination_path: String::from("C:\\Users\\YourUsername\\OneDrive"),
            base_name: String::from("Backup"),
            max_backups: 30,
            compression_level: Some(5),
        }
    }
}

// Pure functions - no side effects
fn build_config_path() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("auto-backup").join("config.toml"))
        .ok_or_else(|| "Could not find config directory".into())
}

fn build_log_path() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|p| p.join("auto-backup").join("backup_log.txt"))
        .ok_or_else(|| "Could not find local data directory".into())
}

fn generate_timestamp() -> String {
    Local::now().format("%Y%m%d_%H%M%S").to_string()
}

fn generate_zip_name(base_name: &str, timestamp: &str) -> String {
    format!("{}_{}.zip", base_name, timestamp)
}

fn validate_path(path: &Path, path_type: &str) -> Result<()> {
    path.exists()
        .then_some(())
        .ok_or_else(|| format!("{} not found at {}", path_type, path.display()).into())
}

// Impure functions - side effects isolated and explicit
fn ensure_directory(path: &Path) -> io::Result<()> {
    path.parent().map(fs::create_dir_all).unwrap_or(Ok(()))
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    ensure_directory(path)?;
    let toml_string = toml::to_string_pretty(config)?;
    fs::write(path, toml_string)?;
    Ok(())
}

fn read_config(path: &Path) -> Result<Config> {
    fs::read_to_string(path)
        .map_err(Into::into)
        .and_then(|content| toml::from_str(&content).map_err(Into::into))
}

fn append_log(path: &Path, message: &str) -> io::Result<()> {
    ensure_directory(path)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{}", message))
}

// Higher-order function for logging with context
fn with_logging<F>(ctx: &BackupContext, message: &str, f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    append_log(&ctx.log_path, message).ok();
    println!("{}", message);
    f()
}

// Functional config loading with Either-like behavior
fn load_or_create_config(config_path: &Path) -> Result<Config> {
    if config_path.exists() {
        read_config(config_path)
    } else {
        let config = Config::default();
        write_config(config_path, &config)?;
        println!("Created default config file at: {}", config_path.display());
        println!("Please edit the config file and run the program again.");
        Err("Config file created with defaults. Please edit it.".into())
    }
}

// Pure function to filter and collect backup files
fn find_backup_files(destination: &Path, base_name: &str) -> Result<Vec<PathBuf>> {
    let pattern = format!("{}_", base_name);

    fs::read_dir(destination)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| name.starts_with(&pattern) && name.ends_with(".zip"))
                .unwrap_or(false)
        })
        .map(|entry| Ok(entry.path()))
        .collect()
}

// Pure function to sort files by modification time
fn sort_by_modified_time(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
    files.sort_by_cached_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok());
    files.reverse();
    files
}

// Pure function to determine which files to delete
fn files_to_delete(files: Vec<PathBuf>, keep: usize) -> Vec<PathBuf> {
    files.into_iter().skip(keep).collect()
}

// Side effect: delete a single file with logging
fn delete_file_logged(path: &PathBuf, log_path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    println!("Deleting: {}", name);
    fs::remove_file(path)?;
    append_log(log_path, &format!("Deleted: {}", name))?;
    Ok(())
}

fn remove_partial_backup(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

// Functional cleanup using composition
fn cleanup_old_backups(ctx: &BackupContext) -> Result<()> {
    let destination = Path::new(&ctx.config.destination_path);
    let backups =
        find_backup_files(destination, &ctx.config.base_name).map(sort_by_modified_time)?;

    println!("Found {} backup files", backups.len());

    if backups.len() > ctx.config.max_backups {
        let to_delete = files_to_delete(backups, ctx.config.max_backups);
        println!("Deleting {} old backup(s)...", to_delete.len());

        to_delete
            .iter()
            .try_for_each(|path| delete_file_logged(path, &ctx.log_path))?;
    } else {
        println!(
            "No cleanup needed. Have {} backups, keeping {}",
            backups.len(),
            ctx.config.max_backups
        );
    }

    Ok(())
}

// Functional zip entry processing - pure operations without logging
fn add_file_to_zip(
    zip: &mut zip::ZipWriter<File>,
    path: &Path,
    name: &Path,
    options: FileOptions,
) -> Result<()> {
    zip.start_file(name.to_string_lossy().to_string(), options)?;
    let mut file = File::open(path)?;
    io::copy(&mut file, zip)?;
    Ok(())
}

fn add_directory_to_zip(
    zip: &mut zip::ZipWriter<File>,
    name: &Path,
    options: FileOptions,
) -> Result<()> {
    zip.add_directory(name.to_string_lossy().to_string(), options)?;
    Ok(())
}

// Process entries with an optional prefix for multi-source backups
// Returns the count of processed items for progress tracking
fn process_entry_with_prefix(
    zip: &mut zip::ZipWriter<File>,
    entry: walkdir::DirEntry,
    source: &Path,
    prefix: &Path,
    options: FileOptions,
) -> Result<usize> {
    let path = entry.path();
    let name = path.strip_prefix(source)?;

    if !name.as_os_str().is_empty() {
        let full_name = prefix.join(name);
        if path.is_file() {
            add_file_to_zip(zip, path, &full_name, options)?;
            Ok(1)
        } else {
            add_directory_to_zip(zip, &full_name, options)?;
            Ok(0)
        }
    } else {
        Ok(0)
    }
}

fn create_zip_backup(ctx: &BackupContext) -> Result<()> {
    let sources = ctx.config.get_source_folders();
    let destination = Path::new(&ctx.config.destination_path);
    let zip_name = generate_zip_name(&ctx.config.base_name, &ctx.timestamp);
    let zip_path = destination.join(&zip_name);

    let file = File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);

    let compression_level = ctx.config.compression_level.unwrap_or(5);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(compression_level as i32))
        .large_file(true);

    let single_source = sources.len() == 1;
    let result: Result<usize> = (|| {
        let mut total_files = 0usize;

        for source_str in &sources {
            let source = Path::new(source_str);

            // For multiple sources, create a folder in the zip with the source's name
            let prefix = if single_source {
                PathBuf::new()
            } else {
                // Use the folder name as the prefix
                source
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_FOLDER_NAME))
            };

            // Accumulate file counts using functional composition with try_fold
            let count = WalkDir::new(source)
                .into_iter()
                .filter_map(|e| e.ok())
                .try_fold(0, |acc, entry| {
                    process_entry_with_prefix(&mut zip, entry, source, &prefix, options)
                        .map(|file_count| acc + file_count)
                })?;

            total_files += count;
        }

        zip.finish()?;
        Ok(total_files)
    })();

    match result {
        Ok(total_files) => {
            println!(
                "Backup created: {} ({} files)",
                zip_path.display(),
                total_files
            );
            Ok(())
        }
        Err(err) => {
            drop(zip);
            remove_partial_backup(&zip_path);
            Err(err)
        }
    }
}

// Build context - gathering all configuration
fn build_context() -> Result<BackupContext> {
    let config_path = build_config_path()?;
    let log_path = build_log_path()?;
    let config = load_or_create_config(&config_path)?;
    let timestamp = generate_timestamp();

    // Warn if both source_folder and source_folders are configured
    if config.source_folder.is_some() && !config.source_folders.is_empty() {
        eprintln!("Warning: Both 'source_folder' and 'source_folders' are configured.");
        eprintln!("Using 'source_folders' and ignoring 'source_folder'.");
        eprintln!();
    }

    Ok(BackupContext {
        config,
        log_path,
        timestamp,
    })
}

// Display functions - pure, side-effect free (returns strings)
fn format_header(ctx: &BackupContext) -> Vec<String> {
    let zip_name = generate_zip_name(&ctx.config.base_name, &ctx.timestamp);
    let sources = ctx.config.get_source_folders();
    let mut lines = vec![
        "Auto-Backup starting...".to_string(),
        String::new(),
        format!("Log file: {}", ctx.log_path.display()),
        String::new(),
    ];

    if sources.len() == 1 {
        lines.push(format!("Source: {}", sources[0]));
    } else {
        lines.push(format!("Sources ({} folders):", sources.len()));
        for source in &sources {
            lines.push(format!("  - {}", source));
        }
    }

    lines.extend(vec![
        format!("Destination: {}", ctx.config.destination_path),
        format!("Backup filename: {}", zip_name),
        format!("Keeping the last {} backups", ctx.config.max_backups),
        String::new(),
    ]);

    lines
}

// Side effect: print lines
fn print_lines(lines: Vec<String>) {
    lines.iter().for_each(|line| println!("{}", line));
}

// Validation pipeline
fn validate_backup_paths(ctx: &BackupContext) -> Result<()> {
    let sources = ctx.config.get_source_folders();
    let destination = Path::new(&ctx.config.destination_path);

    if sources.is_empty() {
        return Err("No source folders configured".into());
    }

    for source in &sources {
        validate_path(Path::new(source), "Source folder")?;
    }

    validate_path(destination, "Destination folder")?;
    Ok(())
}

// Main backup workflow as function composition
fn run_backup(ctx: &BackupContext) -> Result<()> {
    let zip_name = generate_zip_name(&ctx.config.base_name, &ctx.timestamp);

    validate_backup_paths(ctx)?;

    let backup_result = with_logging(ctx, &format!("Creating: {}", zip_name), || {
        println!("Creating zip archive...");
        create_zip_backup(ctx)
    });

    println!();
    let cleanup_result = with_logging(ctx, "Cleaning up old backups...", || {
        cleanup_old_backups(ctx)
    });

    match (backup_result, cleanup_result) {
        (Ok(()), Ok(())) => {
            println!();
            println!("SUCCESS: Backup completed successfully!");
            append_log(&ctx.log_path, "SUCCESS: Backup created").ok();
            append_log(&ctx.log_path, "Backup cleanup completed").ok();
            Ok(())
        }
        (Err(backup_err), Ok(())) => {
            append_log(&ctx.log_path, "Backup cleanup completed").ok();
            Err(backup_err)
        }
        (Ok(()), Err(cleanup_err)) => {
            append_log(&ctx.log_path, "SUCCESS: Backup created").ok();
            append_log(
                &ctx.log_path,
                &format!("Cleanup failed after successful backup: {}", cleanup_err),
            )
            .ok();
            Err(format!(
                "Backup created successfully, but cleanup failed: {}",
                cleanup_err
            )
            .into())
        }
        (Err(backup_err), Err(cleanup_err)) => {
            append_log(
                &ctx.log_path,
                &format!("Cleanup failed after backup error: {}", cleanup_err),
            )
            .ok();
            Err(backup_err)
        }
    }
}

// Error handling as a function
fn handle_error(e: Box<dyn std::error::Error>, config_path: &Path) -> ! {
    eprintln!("Error: {}", e);
    if !config_path.as_os_str().is_empty() {
        eprintln!("Config location: {}", config_path.display());
    }
    std::process::exit(1)
}

fn main() {
    let result = build_context().and_then(|ctx| {
        print_lines(format_header(&ctx));

        let log_start = format!(
            "Backup started at: {}",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        append_log(&ctx.log_path, "==========================================").ok();
        append_log(&ctx.log_path, &log_start).ok();

        run_backup(&ctx).map(|_| ctx)
    });

    match result {
        Ok(ctx) => {
            append_log(&ctx.log_path, "==========================================").ok();
            append_log(&ctx.log_path, "").ok();
            println!();
            println!("Backup script finished.");
            println!("Log file: {}", ctx.log_path.display());
        }
        Err(e) => {
            let config_path = build_config_path().unwrap_or_default();
            handle_error(e, &config_path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn create_test_zip(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"fake zip").unwrap();
        // Sleep briefly so each file gets a distinct modification time
        thread::sleep(Duration::from_millis(10));
        path
    }

    #[test]
    fn test_sort_newest_first() {
        let dir = std::env::temp_dir().join("backup_test_sort");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let _f1 = create_test_zip(&dir, "Backup_20260101_000000.zip");
        let _f2 = create_test_zip(&dir, "Backup_20260102_000000.zip");
        let f3 = create_test_zip(&dir, "Backup_20260103_000000.zip");

        let files = vec![
            dir.join("Backup_20260101_000000.zip"),
            dir.join("Backup_20260102_000000.zip"),
            dir.join("Backup_20260103_000000.zip"),
        ];

        let sorted = sort_by_modified_time(files);
        // Newest should be first
        assert_eq!(sorted[0], f3, "Newest file should be first");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_files_to_delete_skips_newest() {
        let dir = std::env::temp_dir().join("backup_test_delete");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let f1 = create_test_zip(&dir, "Backup_20260101_000000.zip"); // oldest
        let _f2 = create_test_zip(&dir, "Backup_20260102_000000.zip");
        let _f3 = create_test_zip(&dir, "Backup_20260103_000000.zip"); // newest

        let files = vec![
            dir.join("Backup_20260101_000000.zip"),
            dir.join("Backup_20260102_000000.zip"),
            dir.join("Backup_20260103_000000.zip"),
        ];

        let sorted = sort_by_modified_time(files);
        let to_delete = files_to_delete(sorted, 2);

        assert_eq!(to_delete.len(), 1);
        assert_eq!(to_delete[0], f1, "Oldest file should be deleted");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_backup_files_matches_pattern() {
        let dir = std::env::temp_dir().join("backup_test_find");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join("Backup_20260101_000000.zip"), b"").unwrap();
        fs::write(dir.join("Backup_20260102_000000.zip"), b"").unwrap();
        fs::write(dir.join("other_file.txt"), b"").unwrap();
        fs::write(dir.join("NotBackup_20260103_000000.zip"), b"").unwrap();

        let found = find_backup_files(&dir, "Backup").unwrap();
        assert_eq!(found.len(), 2, "Should find exactly 2 backup files");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_no_delete_when_below_limit() {
        let files: Vec<PathBuf> = vec![PathBuf::from("a.zip"), PathBuf::from("b.zip")];
        let max_backups = 3;

        // files.len() (2) <= max_backups (3), so nothing should be deleted
        assert!(files.len() <= max_backups);
        // files_to_delete should return empty when files.len() == max_backups
        let to_delete = files_to_delete(files.clone(), max_backups);
        assert_eq!(to_delete.len(), 0);
    }

    #[test]
    fn test_cleanup_old_backups_keeps_limit() {
        let dir = std::env::temp_dir().join("backup_test_cleanup");
        let log_path = dir.join("backup.log");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        create_test_zip(&dir, "Backup_20260101_000000.zip");
        create_test_zip(&dir, "Backup_20260102_000000.zip");
        create_test_zip(&dir, "Backup_20260103_000000.zip");
        create_test_zip(&dir, "Backup_20260104_000000.zip");
        create_test_zip(&dir, "Backup_20260105_000000.zip");

        let ctx = BackupContext {
            config: Config {
                source_folder: None,
                source_folders: vec![],
                destination_path: dir.to_string_lossy().to_string(),
                base_name: String::from("Backup"),
                max_backups: 3,
                compression_level: Some(5),
            },
            log_path,
            timestamp: String::from("20260105_000000"),
        };

        cleanup_old_backups(&ctx).unwrap();

        let remaining = find_backup_files(&dir, "Backup")
            .map(sort_by_modified_time)
            .unwrap();
        assert_eq!(remaining.len(), 3, "Should keep only 3 backups");

        let remaining_names: Vec<String> = remaining
            .iter()
            .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
            .map(String::from)
            .collect();

        assert_eq!(
            remaining_names,
            vec![
                "Backup_20260105_000000.zip",
                "Backup_20260104_000000.zip",
                "Backup_20260103_000000.zip",
            ]
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_remove_partial_backup_deletes_file() {
        let dir = std::env::temp_dir().join("backup_test_partial_cleanup");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let partial = dir.join("Backup_partial.zip");
        fs::write(&partial, b"incomplete").unwrap();

        remove_partial_backup(&partial);

        assert!(!partial.exists(), "Partial backup should be removed");

        let _ = fs::remove_dir_all(&dir);
    }
}
