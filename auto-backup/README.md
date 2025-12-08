# Auto-Backup

A Rust-based automatic backup utility for Windows 11 that creates timestamped ZIP backups and manages backup rotation.

## Features

- Creates compressed ZIP backups with timestamps
- Configurable source and destination folders
- Automatic cleanup of old backups (keeps only the latest N backups)
- Detailed logging
- Fully configurable via TOML configuration file
- Can be placed in Program Files and run from Windows autostart

## Installation

1. Build the project:
   ```
   cargo build --release
   ```

2. The executable will be in `target/release/auto-backup.exe`

3. Copy `auto-backup.exe` to your desired location (e.g., `C:\Program Files\auto-backup\`)

## Configuration

On first run, the program will create a default configuration file at:
```
%APPDATA%\auto-backup\config.toml
```

Edit this file with your backup settings:

```toml
source_folder = "C:\\Users\\YourName\\Documents"
destination_path = "C:\\Users\\YourName\\OneDrive"
base_name = "Backup"
max_backups = 30
compression_level = 5
```

### Configuration Options

- **source_folder**: The folder you want to backup
- **destination_path**: Where backup ZIP files will be saved
- **base_name**: Prefix for backup filenames (will be appended with timestamp)
- **max_backups**: Number of backups to keep (older ones are automatically deleted)
- **compression_level**: ZIP compression level (0-9, where 5 is default)

## Log Files

Backup logs are stored at:
```
%LOCALAPPDATA%\auto-backup\backup_log.txt
```

## Setting up Autostart

1. Copy `auto-backup.exe` to `C:\Program Files\auto-backup\`
2. Create a shortcut to the executable
3. Press `Win + R`, type `shell:startup`, and press Enter
4. Move the shortcut to the Startup folder that opens

The program will now run automatically when Windows starts.

## Migration from batch script

This program replaces the `backup.bat` script with the following improvements:

- Compiled executable (can be placed in Program Files)
- Configuration file in user space (no need to edit the executable)
- Log file in user's local AppData
- Pure Rust implementation (no external 7-Zip dependency)
- Cross-platform ZIP creation

## Building from Source

Requirements:
- Rust 1.70 or later

```bash
cargo build --release
```

## License

This project is provided as-is for personal use.
