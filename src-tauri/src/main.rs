use serde::Serialize;
use std::{
  fs,
  io::{Seek, SeekFrom, Write},
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
  process::Command,
  sync::{Arc, Mutex},
  thread,
};
use tauri::{Manager, State};

const PROTON_URL: &str = "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/GE-Proton11-6/GE-Proton11-6-x86_64.tar.gz";
const DOTNET_URL: &str = "https://aka.ms/dotnet/8.0/windowsdesktop-runtime-win-x64.exe";
const WEBVIEW_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const INSTALLER_URL: &str = "https://cdn.getmadium.me/Installer-1.1.0.exe";
const OLE32_ORIGINAL_SHA: &str = "505d2c1a337a37f4819f95dad667f7c19bbc3d6235b35a5284423a22b44f98df";
const OLE32_PATCHED_SHA: &str = "16810c72fce5f5aab5ddcd0dbec544a683c77bc6c5aa5aee4cc222ccbdb347da";
const OLE32_PATCH_OFFSET: u64 = 0x34d3b;

const ICON_BYTES: &[u8] = include_bytes!("../icons/128x128.png");

#[derive(Clone, Serialize)]
struct SetupState {
  phase: String,
  current: usize,
  message: String,
  log: String,
  installed: bool,
}
type Shared = Arc<Mutex<SetupState>>;

fn root() -> PathBuf {
  std::env::var_os("XDG_DATA_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share"))
    .join("madlinux")
}

fn update(s: &Shared, current: usize, message: &str) {
  let mut x = s.lock().unwrap();
  x.current = current;
  x.message = message.into();
  x.log.push_str(&format!("\n[{}] {}", current + 1, message));
}

fn sanitize_cmd(cmd: &mut Command) {
  for key in [
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONSAFEPATH",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "GSETTINGS_SCHEMA_DIR",
    "GIO_MODULE_DIR",
    "GIO_EXTRA_MODULES",
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GTK_THEME",
    "GDK_BACKEND",
    "GDK_PIXBUF_MODULE_FILE",
    "APPDIR",
    "APPIMAGE",
    "OWD",
  ] {
    cmd.env_remove(key);
  }
}

fn prepare_proton_cmd(runtime: &Path, compat: &Path, args: &[&str]) -> Command {
  let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
  let steam_client = home.join(".local/share/Steam");
  let _ = fs::create_dir_all(&steam_client);

  let mut cmd = Command::new(runtime.join("proton"));
  sanitize_cmd(&mut cmd);
  cmd.args(args)
    .env("SteamAppId", "0")
    .env("SteamGameId", "0")
    .env("STEAM_COMPAT_DATA_PATH", compat)
    .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_client)
    .env("PROTON_SET_GAME_DRIVE", "0")
    .env("PROTON_SET_STEAM_DRIVE", "0")
    .env("WINEDEBUG", "-all")
    .env("DXVK_LOG_LEVEL", "none");
  cmd
}

fn run(program: &str, args: &[&str], log: &mut String) -> Result<(), String> {
  let mut cmd = Command::new(program);
  sanitize_cmd(&mut cmd);
  let out = cmd
    .args(args)
    .output()
    .map_err(|e| format!("Could not run {program}: {e}"))?;
  log.push_str(&String::from_utf8_lossy(&out.stdout));
  log.push_str(&String::from_utf8_lossy(&out.stderr));
  if out.status.success() {
    Ok(())
  } else {
    Err(format!("{program} exited with {}", out.status))
  }
}

fn run_proton(runtime: &Path, compat: &Path, args: &[&str], log: &mut String) -> Result<(), String> {
  let mut cmd = prepare_proton_cmd(runtime, compat, args);
  let out = cmd.output().map_err(|e| format!("Could not execute proton command: {e}"))?;
  log.push_str(&String::from_utf8_lossy(&out.stdout));
  log.push_str(&String::from_utf8_lossy(&out.stderr));
  if out.status.success() {
    Ok(())
  } else {
    Err(format!("Proton command exited with {}", out.status))
  }
}

fn hash(path: &Path) -> Result<String, String> {
  use sha2::Digest;
  let bytes = fs::read(path).map_err(|e| e.to_string())?;
  Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
  fs::create_dir_all(target).map_err(|e| e.to_string())?;
  for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
    let entry = entry.map_err(|e| e.to_string())?;
    let destination = target.join(entry.file_name());
    if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
      copy_tree(&entry.path(), &destination)?;
    } else {
      fs::copy(entry.path(), destination).map_err(|e| e.to_string())?;
    }
  }
  Ok(())
}

fn contains_file_named(root: &Path, name: &str) -> bool {
  let Ok(entries) = fs::read_dir(root) else { return false };
  entries.flatten().any(|entry| {
    entry
      .file_type()
      .map(|t| {
        if t.is_dir() {
          contains_file_named(&entry.path(), name)
        } else {
          entry.file_name().to_string_lossy().eq_ignore_ascii_case(name)
        }
      })
      .unwrap_or(false)
  })
}

fn find_madium_executable(compat: &Path) -> Option<PathBuf> {
  let candidates = [
    compat.join("pfx/drive_c/users/steamuser/AppData/Local/Madium/Bin/Madium.exe"),
    compat.join("pfx/drive_c/users/steamuser/AppData/Local/Madium/Madium.exe"),
  ];
  if let Some(found) = candidates.iter().find(|p| p.is_file()) {
    return Some((*found).clone());
  }
  let local_madium = compat.join("pfx/drive_c/users/steamuser/AppData/Local/Madium");
  if let Ok(entries) = fs::read_dir(&local_madium) {
    for entry in entries.flatten() {
      let path = entry.path();
      if path.is_file()
        && path
          .file_name()
          .map(|n| n.to_string_lossy().eq_ignore_ascii_case("Madium.exe"))
          .unwrap_or(false)
      {
        return Some(path);
      }
      if path.is_dir() {
        let sub = path.join("Madium.exe");
        if sub.is_file() {
          return Some(sub);
        }
      }
    }
  }
  None
}

fn installer_is_running(compat: &Path) -> bool {
  let prefix = compat.to_string_lossy();
  let Ok(entries) = fs::read_dir("/proc") else { return false };
  entries
    .flatten()
    .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
    .any(|entry| {
      let Ok(bytes) = fs::read(entry.path().join("cmdline")) else { return false };
      let cmd = String::from_utf8_lossy(&bytes).replace('\0', " ");
      (cmd.contains("MadiumInstaller.exe") || cmd.contains("Installer-1.1.0.exe"))
        && (cmd.contains(&*prefix)
          || cmd.contains("AppData/Local/Madium")
          || cmd.contains("AppData\\Local\\Madium"))
    })
}

fn patch_ole32(path: &Path) -> Result<(), String> {
  if !path.exists() {
    return Ok(());
  }
  let digest = hash(path)?;
  if digest == OLE32_PATCHED_SHA {
    return Ok(());
  }
  if digest != OLE32_ORIGINAL_SHA {
    return Err("The compatibility engine has an unexpected ole32.dll version; left unchanged for safety.".into());
  }
  let bytes = fs::read(path).map_err(|e| e.to_string())?;
  if bytes.get(OLE32_PATCH_OFFSET as usize) != Some(&0x74) {
    return Err("The expected WebView compatibility byte was not found; no patch applied.".into());
  }
  let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
  let mut permissions = metadata.permissions();
  permissions.set_mode(permissions.mode() | 0o200);
  fs::set_permissions(path, permissions)
    .map_err(|e| format!("Could not make ole32.dll writable: {e}"))?;
  let mut file = fs::OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
  file.seek(SeekFrom::Start(OLE32_PATCH_OFFSET)).map_err(|e| e.to_string())?;
  file.write_all(&[0xeb]).map_err(|e| e.to_string())?;
  file.flush().map_err(|e| e.to_string())?;
  if hash(path)? != OLE32_PATCHED_SHA {
    return Err("The WebView compatibility fix could not be verified.".into());
  }
  Ok(())
}

fn steps_len() -> usize {
  6
}

#[tauri::command]
fn setup_state(state: State<Shared>) -> SetupState {
  state.lock().unwrap().clone()
}

fn find_bundled_resource(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, String> {
  // 1. Direct and prefixed resource resolution via Tauri
  if let Ok(p) = app.path().resolve(name, tauri::path::BaseDirectory::Resource) {
    if p.exists() {
      return Ok(p);
    }
  }
  let prefixed = format!("resources/{name}");
  if let Ok(p) = app.path().resolve(&prefixed, tauri::path::BaseDirectory::Resource) {
    if p.exists() {
      return Ok(p);
    }
  }

  // 2. Relative to executable location (AppImage mount, Deb /usr/lib, portable directory)
  if let Ok(exe) = std::env::current_exe() {
    if let Some(exe_dir) = exe.parent() {
      let mut candidates = vec![
        exe_dir.join(name),
        exe_dir.join(&prefixed),
      ];
      if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join("lib/MadLinux").join(name));
        candidates.push(parent.join("lib/MadLinux").join(&prefixed));
        candidates.push(parent.join("lib").join(name));
        candidates.push(parent.join("lib").join(&prefixed));
      }
      for c in candidates {
        if c.exists() {
          return Ok(c);
        }
      }
    }
  }

  // 3. Fallback to source directory during development
  let dev_path = PathBuf::from("src-tauri/resources").join(name);
  if dev_path.exists() {
    return Ok(dev_path);
  }

  Err(format!("The bundled resource '{name}' could not be located."))
}

fn download_file(url: &str, target: &Path, log: &mut String) -> Result<(), String> {
  if target.exists() {
    return Ok(());
  }
  if let Some(parent) = target.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let temp = target.with_extension("tmp");
  run(
    "curl",
    &["-L", "--fail", "--retry", "3", "--progress-bar", "-o", temp.to_str().unwrap(), url],
    log,
  )?;
  fs::rename(&temp, target).map_err(|e| format!("Could not save downloaded file: {e}"))?;
  Ok(())
}

fn resolve_or_download(
  app: &tauri::AppHandle,
  name: &str,
  url: &str,
  cache_dir: &Path,
  state: &Shared,
  step: usize,
  label: &str,
) -> Result<PathBuf, String> {
  if let Ok(p) = find_bundled_resource(app, name) {
    if p.exists() {
      return Ok(p);
    }
  }
  let cached = cache_dir.join(name.replace("deps/", ""));
  if cached.exists() {
    return Ok(cached);
  }
  update(state, step, &format!("Downloading {label}…"));
  let mut log = String::new();
  download_file(url, &cached, &mut log)?;
  state.lock().unwrap().log.push_str(&log);
  Ok(cached)
}

#[tauri::command]
fn begin_setup(app: tauri::AppHandle, state: State<Shared>) -> Result<(), String> {
  if state.lock().unwrap().phase == "running" {
    return Ok(());
  }

  {
    let mut x = state.lock().unwrap();
    x.phase = "running".into();
    x.current = 0;
    x.message = "Verifying Linux environment tools".into();
    x.log.clear();
    x.installed = false;
  }

  let shared = state.inner().clone();
  thread::spawn(move || {
    if let Err(e) = install(&app, &shared) {
      let mut x = shared.lock().unwrap();
      x.phase = "error".into();
      x.message = e.clone();
      x.log.push_str(&format!("\nERROR: {e}"));
    }
  });

  Ok(())
}

fn install(app: &tauri::AppHandle, state: &Shared) -> Result<(), String> {
  let base = root();
  fs::create_dir_all(&base).map_err(|e| e.to_string())?;
  let cache_dir = base.join("cache");
  fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

  // Step 0: System check
  update(state, 0, "Checking Linux environment tools");
  for tool in ["curl", "tar"] {
    if Command::new("sh")
      .args(["-c", &format!("command -v {tool}")])
      .status()
      .map_err(|e| e.to_string())?
      .success()
      == false
    {
      return Err(format!("MadLinux needs '{tool}'. Please install it and try again."));
    }
  }

  // Step 1: Compatibility engine
  update(state, 1, "Setting up GE-Proton 11-6 compatibility engine");
  let runtime = base.join("runtime/GE-Proton11-6-MadLinux");
  if !runtime.join("files/bin/wine").exists() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let candidates = [
      home.join(".local/share/Steam/compatibilitytools.d/GE-Proton11-6-x86_64"),
      home.join(".steam/root/compatibilitytools.d/GE-Proton11-6"),
    ];
    fs::create_dir_all(runtime.parent().unwrap()).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&runtime);
    if let Some(src) = candidates.iter().find(|p| p.join("files/bin/wine").exists()) {
      let mut log = String::new();
      run("cp", &["-a", src.to_str().unwrap(), runtime.to_str().unwrap()], &mut log)?;
      state.lock().unwrap().log.push_str(&log);
    } else {
      let archive = cache_dir.join("GE-Proton11-6.tar.gz");
      let mut log = String::new();
      download_file(PROTON_URL, &archive, &mut log)?;
      run(
        "tar",
        &["-xzf", archive.to_str().unwrap(), "-C", runtime.parent().unwrap().to_str().unwrap()],
        &mut log,
      )?;
      let extracted = runtime.parent().unwrap().join("GE-Proton11-6-x86_64");
      fs::rename(extracted, &runtime).map_err(|e| e.to_string())?;
      state.lock().unwrap().log.push_str(&log);
    }
  }

  // Patch ole32.dll for WebView2 stability under Proton
  for relative in [
    "files/lib/wine/x86_64-windows/ole32.dll",
    "files/share/default_pfx/drive_c/windows/system32/ole32.dll",
  ] {
    patch_ole32(&runtime.join(relative))?;
  }

  // Step 2: Private Windows environment
  update(state, 2, "Creating isolated Windows environment");
  let compat = base.join("compatdata");
  fs::create_dir_all(&compat).map_err(|e| e.to_string())?;

  // Stop any lingering wineserver before initialization
  let mut ws_cmd = Command::new(runtime.join("files/bin/wineserver"));
  sanitize_cmd(&mut ws_cmd);
  let _ = ws_cmd.arg("-k").env("WINEPREFIX", compat.join("pfx")).status();

  let pfx = compat.join("pfx");
  if !pfx.join("system.reg").exists() {
    let mut log = String::new();
    run_proton(&runtime, &compat, &["run", "cmd.exe", "/c", "exit"], &mut log)?;
    state.lock().unwrap().log.push_str(&log);
  }

  // Step 3: Web components (.NET 8 and WebView2)
  update(state, 3, "Preparing .NET 8 Desktop Runtime");
  let dotnet_exe = resolve_or_download(
    app,
    "deps/dotnet-8.exe",
    DOTNET_URL,
    &cache_dir,
    state,
    3,
    ".NET 8 Desktop Runtime",
  )?;
  let dotnet_marker = compat.join("pfx/drive_c/Program Files/dotnet/dotnet.exe");
  if !dotnet_marker.exists() {
    update(state, 3, "Installing .NET 8 Desktop Runtime");
    let mut log = String::new();
    run_proton(
      &runtime,
      &compat,
      &["run", dotnet_exe.to_str().unwrap(), "/install", "/quiet", "/norestart"],
      &mut log,
    )?;
    state.lock().unwrap().log.push_str(&log);
  }

  update(state, 3, "Preparing Microsoft Edge WebView2 Runtime");
  let webview_exe = resolve_or_download(
    app,
    "deps/webview2.exe",
    WEBVIEW_URL,
    &cache_dir,
    state,
    3,
    "Microsoft Edge WebView2",
  )?;
  let webview_marker =
    compat.join("pfx/drive_c/Program Files (x86)/Microsoft/EdgeWebView/Application");
  if !contains_file_named(&webview_marker, "msedgewebview2.exe") {
    update(state, 3, "Installing Microsoft Edge WebView2 Runtime");
    let mut log = String::new();
    let _ = run_proton(
      &runtime,
      &compat,
      &["run", webview_exe.to_str().unwrap(), "/silent", "/install"],
      &mut log,
    );
    state.lock().unwrap().log.push_str(&log);
    for _ in 0..120 {
      if contains_file_named(&webview_marker, "msedgewebview2.exe") {
        break;
      }
      thread::sleep(std::time::Duration::from_secs(1));
    }
  }
  if !contains_file_named(&webview_marker, "msedgewebview2.exe") {
    return Err("WebView2 runtime did not finish installing.".into());
  }

  // Seed status.json and dependencies so MadiumInstaller skips its internal downloader
  let madium_deps =
    compat.join("pfx/drive_c/users/steamuser/AppData/Local/Madium/Installer/temp/deps");
  fs::create_dir_all(&madium_deps).map_err(|e| e.to_string())?;
  let _ = fs::copy(&dotnet_exe, madium_deps.join("dotnet-8.exe"));
  let _ = fs::copy(&webview_exe, madium_deps.join("webview2.exe"));
  let status_json = r#"{"done":true,"results":[{"name":".NET 8 Desktop Runtime","code":0,"error":""},{"name":"Microsoft Edge WebView2 Runtime","code":0,"error":""}]}"#;
  fs::write(madium_deps.join("status.json"), status_json).map_err(|e| e.to_string())?;
  state
    .lock()
    .unwrap()
    .log
    .push_str("\nWeb components verified and seeded successfully.");

  // Step 4: Madium installer
  update(state, 4, "Preparing Madium installer");
  let installer_exe = resolve_or_download(
    app,
    "Installer-1.1.0.exe",
    INSTALLER_URL,
    &cache_dir,
    state,
    4,
    "Madium installer",
  )?;

  let inner_dir = compat.join("pfx/drive_c/Installers/MadiumInstaller-1.1.0");
  fs::create_dir_all(&inner_dir).map_err(|e| e.to_string())?;

  if let Ok(payload) = find_bundled_resource(app, "installer-payload") {
    if payload.join("MadiumInstaller.exe").is_file() {
      let _ = copy_tree(&payload, &inner_dir);
    }
  }

  if !inner_dir.join("MadiumInstaller.exe").is_file() {
    update(state, 4, "Extracting Madium installer files");
    let cabextract_bin = runtime.join("protonfixes/files/bin/cabextract");
    let mut log = String::new();
    if cabextract_bin.is_file() {
      let _ = run(
        cabextract_bin.to_str().unwrap(),
        &["-q", "-d", inner_dir.to_str().unwrap(), installer_exe.to_str().unwrap()],
        &mut log,
      );
    } else {
      let _ = run(
        "7z",
        &["x", "-y", &format!("-o{}", inner_dir.to_str().unwrap()), installer_exe.to_str().unwrap()],
        &mut log,
      );
    }
    state.lock().unwrap().log.push_str(&log);
  }

  update(state, 4, "Madium installer window is open — complete setup in that window");
  let run_target = if inner_dir.join("MadiumInstaller.exe").is_file() {
    inner_dir.join("MadiumInstaller.exe")
  } else {
    installer_exe.clone()
  };
  let mut cmd = prepare_proton_cmd(
    &runtime,
    &compat,
    &["run", run_target.to_str().unwrap(), "--from-sfx", "--first-install"],
  );

  let mut child =
    cmd.spawn().map_err(|e| format!("Could not open the Madium installer: {e}"))?;

  thread::sleep(std::time::Duration::from_secs(3));
  while installer_is_running(&compat) {
    if find_madium_executable(&compat).is_some() {
      update(state, 4, "Madium installed! Finishing setup…");
    }
    thread::sleep(std::time::Duration::from_secs(1));
  }
  let _ = child.wait();

  // Step 5: Finishing touches
  update(state, 5, "Verifying your installation and creating shortcuts");
  let madium_exe = find_madium_executable(&compat).ok_or(
    "Madium installation was not completed. Please click 'Install Madium' to finish setup.",
  )?;

  setup_desktop_integration(&base, &runtime, &compat, &madium_exe)?;

  let mut x = state.lock().unwrap();
  x.phase = "done".into();
  x.installed = true;
  x.current = steps_len();
  x.message = "Madium is ready to play!".into();
  x.log
    .push_str("\nSetup completed successfully! Desktop shortcut created in ~/.local/share/applications/madium.desktop.");
  Ok(())
}

fn apply_wine_tweaks(runtime: &Path, compat: &Path) -> Result<(), String> {
  let pfx = compat.join("pfx");
  if !pfx.join("system.reg").exists() {
    return Ok(());
  }
  let tweaks_reg = compat.join("wine-tweaks.reg");
  let reg_content = r#"Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Wine\X11 Driver]
"UsePrimarySelection"="N"

; Keep the game-specific pointer behavior out of Madium's windows. A global
; pointer grab can leave the desktop stuck with a resize cursor after a popup
; is dismissed. Roblox captures the cursor only after it enters fullscreen;
; its chooser therefore keeps a normal, visible cursor.
[HKEY_CURRENT_USER\Software\Wine\AppDefaults\RobloxPlayerBeta.exe\X11 Driver]
"GrabFullscreen"="Y"
"#;
  let _ = fs::write(&tweaks_reg, reg_content);
  let mut log = String::new();
  let _ = run_proton(
    runtime,
    compat,
    &["run", "regedit.exe", "/s", tweaks_reg.to_str().unwrap()],
    &mut log,
  );
  let _ = fs::remove_file(&tweaks_reg);
  Ok(())
}

fn setup_desktop_integration(
  base: &Path,
  runtime: &Path,
  compat: &Path,
  madium_exe: &Path,
) -> Result<PathBuf, String> {
  let home = PathBuf::from(std::env::var_os("HOME").ok_or("Could not locate HOME directory")?);
  let madium_dir = madium_exe.parent().unwrap_or(madium_exe);
  let run_script = base.join("run-madium.sh");

  // Apply Wine clipboard and mouse responsiveness tweaks to prefix
  let _ = apply_wine_tweaks(runtime, compat);

  let script_content = format!(
r#"#!/usr/bin/env bash
set -e

# Sanitize environment so Proton and Python execute cleanly
unset PYTHONHOME PYTHONPATH PYTHONSTARTUP PYTHONSAFEPATH
unset LD_PRELOAD
unset GSETTINGS_SCHEMA_DIR GIO_MODULE_DIR GIO_EXTRA_MODULES
unset GTK_PATH GTK_EXE_PREFIX GTK_DATA_PREFIX GTK_THEME GDK_BACKEND GDK_PIXBUF_MODULE_FILE
unset APPDIR APPIMAGE OWD

export SteamAppId="0"
export SteamGameId="0"
export STEAM_COMPAT_DATA_PATH="${{STEAM_COMPAT_DATA_PATH:-"{}"}}"
export STEAM_COMPAT_CLIENT_INSTALL_PATH="${{STEAM_COMPAT_CLIENT_INSTALL_PATH:-"$HOME/.local/share/Steam"}}"
export PROTON_SET_GAME_DRIVE="0"
export PROTON_SET_STEAM_DRIVE="0"
export WINEDEBUG="-all"
export DXVK_LOG_LEVEL="none"

# Wine optimizations
export PROTON_ENABLE_NVAPI="1"

cd "{}"
exec "{}" run "{}" "$@"
"#,
    compat.display(),
    madium_dir.display(),
    runtime.join("proton").display(),
    madium_exe.display()
  );

  fs::write(&run_script, script_content).map_err(|e| e.to_string())?;
  let _ = Command::new("chmod").args(["+x", run_script.to_str().unwrap()]).status();

  // Install icon
  let icon_dirs = [
    home.join(".local/share/icons/hicolor/128x128/apps"),
    base.to_path_buf(),
  ];
  for dir in icon_dirs {
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("madium.png"), ICON_BYTES);
  }

  // Create desktop entry for app launcher
  let applications_dir = home.join(".local/share/applications");
  fs::create_dir_all(&applications_dir).ok();
  let desktop_file = applications_dir.join("madium.desktop");
  let desktop_content = format!(
r#"[Desktop Entry]
Type=Application
Name=Madium
Comment=Launch Madium (GE-Proton)
Exec="{}"
Icon=madium
Terminal=false
Categories=Game;Development;
StartupNotify=true
StartupWMClass=steam_app_0
"#,
    run_script.display()
  );
  let _ = fs::write(&desktop_file, desktop_content);

  Ok(run_script)
}

#[tauri::command]
fn launch_madium() -> Result<(), String> {
  let base = root();
  let runtime = base.join("runtime/GE-Proton11-6-MadLinux");
  let compat = base.join("compatdata");
  let madium_exe =
    find_madium_executable(&compat).ok_or("Madium executable not found. Please run setup first.")?;

  let run_script = setup_desktop_integration(&base, &runtime, &compat, &madium_exe)?;
  let madium_dir = madium_exe.parent().unwrap_or(&madium_exe);

  let mut cmd = Command::new(&run_script);
  sanitize_cmd(&mut cmd);
  cmd.current_dir(madium_dir);
  cmd.stdin(std::process::Stdio::null());
  cmd.stdout(std::process::Stdio::null());
  cmd.stderr(std::process::Stdio::null());

  cmd.spawn().map_err(|e| format!("Could not start Madium: {e}"))?;
  Ok(())
}

#[tauri::command]
fn close_window(app: tauri::AppHandle) {
  if let Some(w) = app.get_webview_window("main") {
    let _ = w.close();
  }
}

fn main() {
  std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
  let base = root();
  let runtime = base.join("runtime/GE-Proton11-6-MadLinux");
  let compat = base.join("compatdata");
  let madium_exe = find_madium_executable(&compat);
  let installed = madium_exe.is_some();
  if let Some(ref exe) = madium_exe {
    let _ = setup_desktop_integration(&base, &runtime, &compat, exe);
  }
  let state = Arc::new(Mutex::new(SetupState {
    phase: if installed { "done" } else { "waiting" }.into(),
    current: if installed { steps_len() } else { 0 },
    message: if installed {
      "Madium is ready to play!"
    } else {
      "Everything needed to install Madium is included."
    }
    .into(),
    log: String::new(),
    installed,
  }));
  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![
      setup_state,
      begin_setup,
      launch_madium,
      close_window
    ])
    .run(tauri::generate_context!())
    .expect("failed to run MadLinux");
}
