use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::CompressionMethod;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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
    path.parent()
        .map(|p| fs::create_dir_all(p))
        .unwrap_or(Ok(()))
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
            entry.file_name()
                .to_str()
                .map(|name| name.starts_with(&pattern) && name.ends_with(".zip"))
                .unwrap_or(false)
        })
        .map(|entry| Ok(entry.path()))
        .collect()
}

// Pure function to sort files by modification time
fn sort_by_modified_time(mut files: Vec<PathBuf>) -> Vec<PathBuf> {
    files.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
    });
    files.reverse();
    files
}

// Pure function to determine which files to delete
fn files_to_delete(files: Vec<PathBuf>, keep: usize) -> Vec<PathBuf> {
    files.into_iter().skip(keep).collect()
}

// Side effect: delete a single file with logging
fn delete_file_logged(path: &PathBuf, log_path: &Path) -> Result<()> {
    let name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    
    println!("Deleting: {}", name);
    fs::remove_file(path)?;
    append_log(log_path, &format!("Deleted: {}", name))?;
    Ok(())
}

// Functional cleanup using composition
fn cleanup_old_backups(ctx: &BackupContext) -> Result<()> {
    let destination = Path::new(&ctx.config.destination_path);
    let backups = find_backup_files(destination, &ctx.config.base_name)
        .map(sort_by_modified_time)?;
    
    println!("Found {} backup files", backups.len());
    
    if backups.len() > ctx.config.max_backups {
        let to_delete = files_to_delete(backups, ctx.config.max_backups);
        println!("Deleting {} old backup(s)...", to_delete.len());
        
        to_delete.iter()
            .try_for_each(|path| delete_file_logged(path, &ctx.log_path))?;
    } else {
        println!("No cleanup needed. Have {} backups, keeping {}", 
                 backups.len(), ctx.config.max_backups);
    }
    
    Ok(())
}

// Functional zip entry processing
fn add_file_to_zip(
    zip: &mut zip::ZipWriter<File>,
    path: &Path,
    name: &Path,
    options: FileOptions,
) -> Result<()> {
    println!("Adding: {}", name.display());
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
    println!("Adding directory: {}", name.display());
    zip.add_directory(name.to_string_lossy().to_string(), options)?;
    Ok(())
}

// Process entries with an optional prefix for multi-source backups
fn process_entry_with_prefix(
    zip: &mut zip::ZipWriter<File>,
    entry: walkdir::DirEntry,
    source: &Path,
    prefix: &Path,
    options: FileOptions,
) -> Result<()> {
    let path = entry.path();
    let name = path.strip_prefix(source)?;
    
    if !name.as_os_str().is_empty() {
        let full_name = prefix.join(name);
        if path.is_file() {
            add_file_to_zip(zip, path, &full_name, options)
        } else {
            add_directory_to_zip(zip, &full_name, options)
        }
    } else {
        Ok(())
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
        .compression_level(Some(compression_level as i32));
    
    let single_source = sources.len() == 1;
    
    for source_str in &sources {
        let source = Path::new(source_str);
        
        // For multiple sources, create a folder in the zip with the source's name
        let prefix = if single_source {
            PathBuf::new()
        } else {
            // Use the folder name as the prefix
            source.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("folder"))
        };
        
        WalkDir::new(source)
            .into_iter()
            .filter_map(|e| e.ok())
            .try_for_each(|entry| process_entry_with_prefix(&mut zip, entry, source, &prefix, options))?;
    }
    
    zip.finish()?;
    println!("Backup created: {}", zip_path.display());
    Ok(())
}

// Build context - gathering all configuration
fn build_context() -> Result<BackupContext> {
    let config_path = build_config_path()?;
    let log_path = build_log_path()?;
    let config = load_or_create_config(&config_path)?;
    let timestamp = generate_timestamp();
    
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
    
    with_logging(ctx, &format!("Creating: {}", zip_name), || {
        println!("Creating zip archive...");
        create_zip_backup(ctx)
    })?;
    
    println!();
    println!("SUCCESS: Backup completed successfully!");
    append_log(&ctx.log_path, "SUCCESS: Backup created").ok();
    
    println!();
    with_logging(ctx, "Cleaning up old backups...", || {
        cleanup_old_backups(ctx)
    })?;
    
    append_log(&ctx.log_path, "Backup cleanup completed").ok();
    Ok(())
}

// Error handling as a function
fn handle_error(e: Box<dyn std::error::Error>, config_path: &Path) -> ! {
    eprintln!("Error loading config: {}", e);
    eprintln!("Config location: {}", config_path.display());
    std::process::exit(1)
}

fn main() {
    let result = build_context()
        .and_then(|ctx| {
            print_lines(format_header(&ctx));
            
            let log_start = format!("Backup started at: {}", 
                Local::now().format("%Y-%m-%d %H:%M:%S"));
            append_log(&ctx.log_path, "==========================================").ok();
            append_log(&ctx.log_path, &log_start).ok();
            
            run_backup(&ctx)
                .map(|_| ctx)
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
