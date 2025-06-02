use std::{env, fs};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn setup_persistent_daemon() -> crate::Result<()> {
    // Step 1: Kill any existing daemon processes
    println!("Step 1: Killing existing daemon processes...");
    kill_existing_daemons()?;

    // Step 2: Copy executable to permanent location
    println!("Step 2: Copying executable to permanent location...");
    let daemon_exe_path = copy_exe_to_permanent_location()?;

    // Step 3: Remove old startup entries
    println!("Step 3: Removing old startup entries...");
    remove_old_startup_entries()?;

    // Step 4: Create new startup scripts
    println!("Step 4: Creating new startup scripts...");
    create_startup_scripts(&daemon_exe_path)?;

    // Step 5: Launch the daemon
    println!("Step 5: Launching the daemon...");
    launch_daemon(&daemon_exe_path)?;

    Ok(())
}

fn kill_existing_daemons() -> crate::Result<()> {
    #[cfg(windows)]
    {
        // Kill only daemon processes (those with --daemon argument)
        let output = Command::new("wmic")
            .args(["process", "where", "name='augment-vip.exe' and commandline like '%--daemon%'", "get", "processid", "/format:value"])
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    if line.starts_with("ProcessId=") {
                        if let Some(pid_str) = line.strip_prefix("ProcessId=") {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                let _ = Command::new("taskkill")
                                    .args(["/PID", &pid.to_string(), "/F"])
                                    .output();
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    {
        let _ = Command::new("pkill")
            .args(["-f", "augment-vip.*--daemon"])
            .output();
    }

    println!("Cleaned up existing daemon processes");
    Ok(())
}

fn copy_exe_to_permanent_location() -> crate::Result<PathBuf> {
    let current_exe = env::current_exe()?;

    #[cfg(windows)]
    let target_dir = dirs::data_local_dir()
        .ok_or("Could not find local data directory")?
        .join("AugmentVip");

    #[cfg(unix)]
    let target_dir = dirs::data_local_dir()
        .ok_or("Could not find local data directory")?
        .join("augment-vip");

    fs::create_dir_all(&target_dir)?;

    let target_exe = target_dir.join(current_exe.file_name().unwrap());

    // Delete existing executable if it exists
    if target_exe.exists() {
        fs::remove_file(&target_exe)?;
        println!("Removed existing executable: {}", target_exe.display());
    }

    // Copy the new executable
    fs::copy(&current_exe, &target_exe)?;

    println!("Copied executable to: {}", target_exe.display());
    Ok(target_exe)
}

fn remove_old_startup_entries() -> crate::Result<()> {
    #[cfg(windows)]
    {
        let startup_folder = get_windows_startup_folder()?;
        let patterns = ["augment-vip-daemon.bat", "Augment VIP Daemon.lnk", "augment-vip.bat"];

        for pattern in &patterns {
            let path = startup_folder.join(pattern);
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let autostart_dir = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("autostart");

        let patterns = ["augment-vip-daemon.desktop", "augment-vip.desktop"];
        for pattern in &patterns {
            let path = autostart_dir.join(pattern);
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let launch_agents_dir = dirs::home_dir()
            .ok_or("Could not find home directory")?
            .join("Library")
            .join("LaunchAgents");

        let patterns = ["com.augment.vip.daemon.plist", "com.augment.vip.plist"];
        for pattern in &patterns {
            let path = launch_agents_dir.join(pattern);
            if path.exists() {
                let _ = Command::new("launchctl")
                    .args(["unload", &path.to_string_lossy()])
                    .status();
                let _ = fs::remove_file(&path);
            }
        }
    }

    println!("Removed old startup entries");
    Ok(())
}

fn get_windows_startup_folder() -> crate::Result<PathBuf> {
    #[cfg(windows)]
    {
        let startup_folder = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup");

        if startup_folder.exists() {
            return Ok(startup_folder);
        }

        // Alternative path
        let appdata = env::var("APPDATA")
            .map_err(|_| "Could not find APPDATA environment variable")?;
        let alt_startup = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup");

        if alt_startup.exists() {
            Ok(alt_startup)
        } else {
            Err("Could not find Windows startup folder".into())
        }
    }

    #[cfg(not(windows))]
    {
        Err("Windows startup folder not available on this platform".into())
    }
}

fn create_startup_scripts(daemon_exe_path: &Path) -> crate::Result<()> {
    #[cfg(windows)]
    {
        let startup_folder = get_windows_startup_folder()?;
        let batch_path = startup_folder.join("augment-vip-daemon.bat");
        let batch_content = format!(
            "@echo off\nstart /min \"\" \"{}\" --daemon",
            daemon_exe_path.display()
        );
        fs::write(&batch_path, batch_content)?;
        println!("Created startup script: {}", batch_path.display());
    }

    #[cfg(target_os = "linux")]
    {
        let autostart_dir = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("autostart");

        fs::create_dir_all(&autostart_dir)?;

        let desktop_file_path = autostart_dir.join("augment-vip-daemon.desktop");
        let desktop_content = format!(
            r#"[Desktop Entry]
Type=Application
Name=Augment VIP Daemon
Comment=Monitors and protects VSCode telemetry settings
Exec={} --daemon
Hidden=false
NoDisplay=false
X-GNOME-Autostart-enabled=true
"#,
            daemon_exe_path.display()
        );

        fs::write(&desktop_file_path, desktop_content)?;

        // Make it executable
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&desktop_file_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&desktop_file_path, perms)?;

        println!("Created autostart entry: {}", desktop_file_path.display());
    }

    #[cfg(target_os = "macos")]
    {
        let launch_agents_dir = dirs::home_dir()
            .ok_or("Could not find home directory")?
            .join("Library")
            .join("LaunchAgents");

        fs::create_dir_all(&launch_agents_dir)?;

        let plist_path = launch_agents_dir.join("com.augment.vip.daemon.plist");
        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.augment.vip.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/dev/null</string>
    <key>StandardErrorPath</key>
    <string>/dev/null</string>
</dict>
</plist>
"#,
            daemon_exe_path.display()
        );

        fs::write(&plist_path, plist_content)?;

        // Load the launch agent
        let load_result = Command::new("launchctl")
            .args(["load", &plist_path.to_string_lossy()])
            .status()?;

        if load_result.success() {
            println!("Created and loaded launch agent: {}", plist_path.display());
        } else {
            println!("Created launch agent: {} (will start on next login)", plist_path.display());
        }
    }

    Ok(())
}

fn launch_daemon(daemon_exe_path: &Path) -> crate::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        let child = Command::new(daemon_exe_path)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()?;

        println!("Launched daemon with PID: {}", child.id());
    }

    #[cfg(unix)]
    {
        use std::process::Stdio;
        let child = Command::new(daemon_exe_path)
            .arg("--daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        println!("Launched daemon with PID: {}", child.id());
    }

    Ok(())
}

pub fn uninstall_daemon() -> crate::Result<()> {
    println!("🗑️  Uninstalling Augment VIP daemon...");

    // Step 1: Kill all running daemon processes
    println!("Step 1: Stopping daemon processes...");
    kill_existing_daemons()?;

    // Step 2: Remove startup entries
    println!("Step 2: Removing startup entries...");
    remove_startup_entries()?;

    // Step 3: Remove installed executable
    println!("Step 3: Removing installed files...");
    remove_installed_executable()?;

    println!("🧹 Cleanup completed!");
    Ok(())
}

fn remove_startup_entries() -> crate::Result<()> {
    #[cfg(windows)]
    {
        let startup_folder = get_windows_startup_folder()?;
        let patterns = ["augment-vip-daemon.bat", "Augment VIP Daemon.lnk", "augment-vip.bat"];

        let mut removed_count = 0;
        for pattern in &patterns {
            let path = startup_folder.join(pattern);
            if path.exists() {
                fs::remove_file(&path)?;
                println!("  Removed: {}", path.display());
                removed_count += 1;
            }
        }

        if removed_count == 0 {
            println!("  No startup entries found to remove");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let autostart_dir = dirs::config_dir()
            .ok_or("Could not find config directory")?
            .join("autostart");

        let patterns = ["augment-vip-daemon.desktop", "augment-vip.desktop"];
        let mut removed_count = 0;

        for pattern in &patterns {
            let path = autostart_dir.join(pattern);
            if path.exists() {
                fs::remove_file(&path)?;
                println!("  Removed: {}", path.display());
                removed_count += 1;
            }
        }

        if removed_count == 0 {
            println!("  No autostart entries found to remove");
        }
    }

    #[cfg(target_os = "macos")]
    {
        let launch_agents_dir = dirs::home_dir()
            .ok_or("Could not find home directory")?
            .join("Library")
            .join("LaunchAgents");

        let patterns = ["com.augment.vip.daemon.plist", "com.augment.vip.plist"];
        let mut removed_count = 0;

        for pattern in &patterns {
            let path = launch_agents_dir.join(pattern);
            if path.exists() {
                // Unload the agent first
                let _ = Command::new("launchctl")
                    .args(["unload", &path.to_string_lossy()])
                    .status();

                fs::remove_file(&path)?;
                println!("  Removed: {}", path.display());
                removed_count += 1;
            }
        }

        if removed_count == 0 {
            println!("  No launch agents found to remove");
        }
    }

    Ok(())
}

fn remove_installed_executable() -> crate::Result<()> {
    #[cfg(windows)]
    let target_dir = dirs::data_local_dir()
        .ok_or("Could not find local data directory")?
        .join("AugmentVip");

    #[cfg(unix)]
    let target_dir = dirs::data_local_dir()
        .ok_or("Could not find local data directory")?
        .join("augment-vip");

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
        println!("  Removed directory: {}", target_dir.display());
    } else {
        println!("  No installed files found to remove");
    }

    Ok(())
}