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
%APPDATA%\Roaming\auto-backup\config.toml
```

Edit this file with your backup settings:

```toml
# Single folder backup (files will be at the root of the zip)
source_folders = ["C:\\Users\\YourName\\Documents"]

# Or multiple folders (each in a separate folder in the zip)
source_folders = [
    "C:\\Users\\YourName\\Documents",
    "C:\\Users\\YourName\\Pictures"
]

destination_path = "C:\\Users\\YourName\\OneDrive"
base_name = "Backup"
max_backups = 30
compression_level = 5
```

### Configuration Options

- **source_folders**: List of folders you want to backup. For a single folder, the files will be at the root of the zip. For multiple folders, each will be in a separate folder within the zip.
- **destination_path**: Where backup ZIP files will be saved
- **base_name**: Prefix for backup filenames (will be appended with timestamp)
- **max_backups**: Number of backups to keep (older ones are automatically deleted)
- **compression_level**: ZIP compression level (0-9, where 5 is default)

**Note**: For backward compatibility, you can still use `source_folder` (singular) for a single folder backup.

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

## Building from Source

Requirements:
- Rust 1.70 or later

```bash
cargo build --release
```

## License

This project is provided as-is for personal use.
