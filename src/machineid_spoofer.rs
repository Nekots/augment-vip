pub fn spoof() -> Result<(), Box<dyn std::error::Error>> {
    println!("Spoofing machine IDs...");
    #[cfg(target_os = "windows")]
    return spoof_windows();
    #[cfg(target_os = "linux")]
    return spoof_linux();
    #[cfg(target_os = "macos")]
    return spoof_macos();
}

// Changes MachineGuid in registry HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography
#[cfg(target_os = "windows")]
fn spoof_windows() -> Result<(), Box<dyn std::error::Error>> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
    use winreg::RegKey;
    use uuid::Uuid;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let crypto_key = hklm.open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", KEY_READ | KEY_WRITE)?;
    let old_guid: String = crypto_key.get_value("MachineGuid")?;
    let new_guid = Uuid::new_v4().to_string();
    if !crypto_key.get_value::<String, _>("OriginalMachineGuid").is_ok() { crypto_key.set_value("OriginalMachineGuid", &old_guid)?; }
    crypto_key.set_value("MachineGuid", &new_guid)?;
    println!("MachineGuid changed from {} to {}", old_guid, new_guid);
    Ok(())
}

// Changes `/var/lib/dbus/machine-id` and `/etc/machine-id`. If reading both fails then augment resorts to `hostname`
#[cfg(target_os = "linux")]
fn spoof_linux() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::{self, File};
    use std::io::{Read};
    use uuid::Uuid;

    let paths = ["/var/lib/dbus/machine-id", "/etc/machine-id"];
    let new_id = Uuid::new_v4().to_string().replace("-", "");
    let mut old_id = String::new();
    for path in paths {
        if let Ok(mut file) = File::open(path) {
            if file.read_to_string(&mut old_id).is_err() { continue; }
            let backup_path = format!("{}.original", path);
            if !std::path::Path::new(&backup_path).exists() { fs::write(&backup_path, &old_id)?; }
            fs::write(path, &new_id)?;
            println!("Machine ID at {} changed from {} to {}", path, old_id, new_id);
            return Ok(());
        }
    }
    Err("Failed to change machine ID".into())
    // TODO: Immutable Linux Systems
    // TODO: Change hostname without altering things too much
}

// Attempts to change the platform uuid on macOS (idk if this'll work lol)
#[cfg(target_os = "macos")]
fn spoof_macos() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::process::Command;
    use uuid::Uuid;

    // Generate new UUID
    let new_uuid = Uuid::new_v4().to_string();

    // Get current UUID from IOPlatformExpertDevice
    let output = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut old_uuid = String::new();

    // Extract UUID from output
    for line in output_str.lines() {
        if line.contains("IOPlatformUUID") {
            if let Some(uuid_part) = line.split("\"").nth(3) {
                old_uuid = uuid_part.to_string();
                break;
            }
        }
    }

    if old_uuid.is_empty() {
        return Err("Failed to find current UUID".into());
    }

    // Create nvram.plist backup if it doesn't exist
    let nvram_path = "/var/db/nvram.plist";
    let backup_path = "/var/db/nvram.plist.original";

    if !std::path::Path::new(backup_path).exists() && std::path::Path::new(nvram_path).exists() {
        fs::copy(nvram_path, backup_path)?;
        println!("Created backup of nvram.plist at {}", backup_path);
    }

    // Set new UUID using nvram
    let _ = Command::new("sudo")
        .args(["nvram", &format!("platform-uuid={}", new_uuid)])
        .status()?;

    println!("Platform UUID changed from {} to {}", old_uuid, new_uuid);

    // Note: Changes will take effect after restart
    println!("Note: A system restart is required for changes to take effect");

    Ok(())
}
