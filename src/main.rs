use base64::{Engine as _, engine::general_purpose};
use clap::Parser;
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::fs::{self};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use default_args::default_args;
use kill_tree::blocking::kill_tree;
use sysinfo::System;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(name = "augment-vip")]
#[command(about = "A tool for managing JetBrains and VSCode telemetry IDs")]
struct Args {
    /// Skip all pauses for user interaction
    #[arg(long)]
    no_pause: bool,

    /// Skip signing out of accounts
    #[arg(long)]
    no_signout: bool,
    
    /// Skip IDE termination
    #[arg(long)]
    no_terminate: bool,
}

fn main() {
    let args = Args::parse();
    
    #[cfg(target_os = "macos")]
    if let Err(e) = sudo2::escalate_if_needed() { // Request sudo permissions early on macOS
        eprintln!("Warning: {}\n\nFile locking may not work properly!", e);
    }

    if let Err(e) = run(&args) {
        eprintln!("Error: {}", e);
        pause(&args);
        std::process::exit(1);
    }

    pause(&args);
}

fn pause(args: &Args) {
    if args.no_pause { return; }
    print!("\nPress Enter to exit...");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut String::new()).unwrap();
}

fn terminate_ides() {
    for (pid, process) in System::new_all().processes() {
        let cmd_str = process.cmd().join(" ".as_ref()).to_string_lossy().to_string();
        if !cmd_str.contains("vscode") && !cmd_str.contains(".augmentcode") { continue; }
        if let Some(parent_pid) = process.parent() {
            let _ = kill_tree(parent_pid.as_u32());
        }
        let _ = kill_tree(pid.as_u32());
    }
    // TODO: Restart IDEs
}

fn get_jetbrains_config_dir() -> Option<PathBuf> {
    [dirs::config_dir(), dirs::home_dir(), dirs::data_dir()]
        .into_iter()
        .filter_map(|base_dir| base_dir)
        .map(|base_dir| base_dir.join("JetBrains"))
        .find(|path| path.exists())
}

fn get_vscode_files(id: &str) -> Option<Vec<PathBuf>> {
    let base_dirs = [dirs::config_dir(), dirs::home_dir(), dirs::data_dir()];
    let global_patterns = [
        &["User", "globalStorage"] as &[&str],
        &["data", "User", "globalStorage"],
        &[id],
        &["data", id],
    ];
    let workspace_patterns = [
        &["User", "workspaceStorage"] as &[&str],
        &["data", "User", "workspaceStorage"],
    ];

    let vscode_dirs: Vec<PathBuf> = base_dirs
        .into_iter()
        .filter_map(|base_dir| base_dir)
        .flat_map(|base_dir| {
            fs::read_dir(&base_dir)
                .into_iter()
                .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
                .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .flat_map(|entry| {
                    let entry_path = entry.path();

                    // Global storage patterns
                    let global_paths: Vec<PathBuf> = global_patterns.iter().map(|pattern| {
                        pattern.iter().fold(entry_path.clone(), |path, segment| path.join(segment))
                    }).collect();

                    // Workspace storage patterns - enumerate all subdirectories
                    let workspace_paths: Vec<PathBuf> = workspace_patterns.iter().flat_map(|pattern| {
                        let workspace_base = pattern.iter().fold(entry_path.clone(), |path, segment| path.join(segment));
                        if workspace_base.exists() {
                            fs::read_dir(&workspace_base)
                                .into_iter()
                                .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
                                .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                                .map(|entry| entry.path())
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        }
                    }).collect();

                    global_paths.into_iter().chain(workspace_paths)
                })
        })
        .filter(|path| path.exists())
        .collect();

    (!vscode_dirs.is_empty()).then_some(vscode_dirs)
}

fn update_id_file(file_path: &Path) -> Result<()> {
    println!("Updating file: {}", file_path.display());

    // Show old UUID if it exists
    if file_path.exists() {
        let old_uuid = fs::read_to_string(file_path).unwrap_or_default();
        if !old_uuid.is_empty() {
            println!("Old UUID: {}", old_uuid);
        }
    }

    // Show new UUID
    let new_uuid = Uuid::new_v4().to_string();
    println!("New UUID: {}", new_uuid);

    // Delete the file if it exists
    if file_path.exists() {
        let _ = fs::remove_file(file_path);
        
        #[cfg(target_os = "macos")]
        let _ = Command::new("sudo")
            .arg("rm")
            .arg("-rf")
            .arg(&file_path.to_string_lossy().to_string())
            .status();
    }

    // Write new UUID to file
    fs::write(file_path, new_uuid)?;

    println!("Successfully wrote new UUID to file");
    Ok(())
}

fn update_vscode_files(vscode_file_path: &Path, vscode_keys: &[&str; 4]) -> Result<()> {
    let storage_json_path = vscode_file_path.join("storage.json");
    
    if storage_json_path.exists() {
        println!("Updating VSCode storage: {}", storage_json_path.display());

        // Read existing storage.json or create empty object
        let mut data: Map<String, Value> = storage_json_path.exists()
            .then(|| fs::read_to_string(&storage_json_path).ok())
            .flatten()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_else(Map::new);

        for key_encoded in vscode_keys {
            let key = String::from_utf8(general_purpose::STANDARD.decode(key_encoded)?)?;

            // Show old value if it exists
            if let Some(old_value) = data.get(&key) {
                println!("Old UUID: {}", old_value.as_str().unwrap_or_default());
            }

            // Generate and update new value
            let new_value = if *key_encoded == "dGVsZW1ldHJ5LmRldkRldmljZUlk" {
                Uuid::new_v4().to_string() // ... (something something look into something something) ...
            } else {
                format!("{:x}", Sha256::digest(Uuid::new_v4().as_bytes())) // Some fields are SHA-256 hashes
            };
            println!("New UUID: {}", new_value);
            data.insert(key, Value::String(new_value));
        }

        // Write back to file
        let json_content = serde_json::to_string_pretty(&data)?;
        fs::write(&storage_json_path, json_content)?;

        println!("Successfully wrote new UUIDs to file");
    }
    
    if vscode_file_path.exists() && vscode_file_path.is_file() { // it's the id file
        update_id_file(vscode_file_path)?;
        lock_file(vscode_file_path)?;
    }
    
    Ok(()) // continue
}

default_args! {
    fn clean_vscode_database(vscode_global_storage_path: &Path, count_query: &String, delete_query: &String, file_name: &String = &"state.vscdb".to_string()) -> Result<()> {
        let state_db_path = vscode_global_storage_path.join(file_name);
    
        if !state_db_path.exists() {
            return Ok(());
        }
    
        let conn = Connection::open(&state_db_path)?;
    
        // Check how many rows would be deleted first
        let rows_to_delete: i64 = conn.prepare(count_query)?.query_row([], |row| row.get(0))?;
        if rows_to_delete > 0 {
            println!("Found {} potential entries to remove from '{}'", rows_to_delete, state_db_path.file_name().unwrap_or_default().to_string_lossy());
    
            // Execute the delete query
            conn.execute(delete_query, [])?;
    
            println!("Successfully removed {} entries from '{}'", rows_to_delete, state_db_path.file_name().unwrap_or_default().to_string_lossy());
        }
    
        if file_name.ends_with(".backup") {
            return Ok(());
        }
        clean_vscode_database_(vscode_global_storage_path, count_query, delete_query, &(file_name.to_string() + ".backup"))
    }
}

fn run(args: &Args) -> Result<()> {
    if !args.no_terminate { terminate_ides(); }
    let mut programs_found = false;

    // Try to find and update JetBrains
    if let Some(jetbrains_dir) = get_jetbrains_config_dir() {
        programs_found = true;

        let id_files = ["UGVybWFuZW50RGV2aWNlSWQ=", "UGVybWFuZW50VXNlcklk"];

        for file_name in &id_files {
            let file_path = jetbrains_dir.join(String::from_utf8(general_purpose::STANDARD.decode(file_name)?)?);
            update_id_file(&file_path)?;
            lock_file(&file_path)?;
        }

        println!("JetBrains ID files have been updated and locked successfully!");
    } else {
        println!("JetBrains configuration directory not found");
    }

    // Try to find and update VSCode variants
    if let Some(vscode_dirs) = get_vscode_files(&String::from_utf8(general_purpose::STANDARD.decode("bWFjaGluZUlk")?)?) {
        programs_found = true;

        let vscode_keys = ["dGVsZW1ldHJ5Lm1hY2hpbmVJZA==", "dGVsZW1ldHJ5LmRldkRldmljZUlk", "dGVsZW1ldHJ5Lm1hY01hY2hpbmVJZA==", "c3RvcmFnZS5zZXJ2aWNlTWFjaGluZUlk"];
        let count_query = String::from_utf8(general_purpose::STANDARD.decode("U0VMRUNUIENPVU5UKCopIEZST00gSXRlbVRhYmxlIFdIRVJFIGtleSBMSUtFICclYXVnbWVudCUnOw==")?)?;
        let delete_query = String::from_utf8(general_purpose::STANDARD.decode("REVMRVRFIEZST00gSXRlbVRhYmxlIFdIRVJFIGtleSBMSUtFICclYXVnbWVudCUnOw==")?)?;

        for vscode_dir in vscode_dirs {
            update_vscode_files(&vscode_dir, &vscode_keys)?;
            if !args.no_signout { clean_vscode_database!(&vscode_dir, &count_query, &delete_query)?; }
        }

        println!("All found VSCode variants' ID files have been updated and databases cleaned successfully!");
    } else {
        println!("No VSCode variants found");
    }

    // Error only if no programs were found at all
    if !programs_found {
        return Err("No JetBrains or VSCode installations found".into());
    }
    
    Ok(())
}

fn lock_file(file_path: &Path) -> Result<()> {
    println!("Locking file: {}", file_path.display());

    if !file_path.exists() {
        return Err(format!("File doesn't exist, can't lock: {}", file_path.display()).into());
    }

    // Use platform-specific commands to lock the file
    if cfg!(windows) {
        let _ = Command::new("attrib")
            .args(["+R", &file_path.to_string_lossy()])
            .status();
    } else {
        let _ = Command::new("chmod")
            .args(["444", &file_path.to_string_lossy()])
            .status();

        #[cfg(target_os = "macos")]
        let _ = Command::new("sudo")
            .args(["chflags", "uchg", &file_path.to_string_lossy()])
            .status();
    }

    // Always ensure file is read-only using Rust API regardless of platform command result
    #[cfg(not(target_os = "macos"))] // Rust's filesystem api doesn't work on mac
    {
        let mut perms = fs::metadata(file_path)?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(file_path, perms)?;
    }

    println!("Successfully locked file");
    Ok(())
}
