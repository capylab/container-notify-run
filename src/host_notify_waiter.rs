use distrobox_run_notify::SimpleLogger;
use log::{error, info, LevelFilter};
use std::env;
use std::fs;
use std::process::{self, Command};
use std::thread;
use std::time::{Duration, Instant};

fn notify_ready() -> Result<(), Box<dyn std::error::Error>> {
    info!("Sending READY=1 notification to systemd");
    sd_notify::notify(false, &[sd_notify::NotifyState::Ready])?;
    Ok(())
}

fn notify_stopping() -> Result<(), Box<dyn std::error::Error>> {
    info!("Sending STOPPING=1 notification to systemd");
    sd_notify::notify(false, &[sd_notify::NotifyState::Stopping])?;
    Ok(())
}

fn notify_watchdog() -> Result<(), Box<dyn std::error::Error>> {
    info!("Sending WATCHDOG=1 notification to systemd");
    sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog])?;
    Ok(())
}

fn notify_status(status: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!("Sending STATUS={} notification to systemd", status);
    sd_notify::notify(false, &[sd_notify::NotifyState::Status(status)])?;
    Ok(())
}

fn main() {
    let mut args: Vec<String> = env::args().collect();
    
    // Check for verbose flag
    let verbose = args.contains(&"--verbose".to_string());
    if verbose {
        args.retain(|x| x != "--verbose");
    }
    
    // Initialize logger
    let log_level = if verbose {
        LevelFilter::Info
    } else {
        LevelFilter::Error
    };
    SimpleLogger::init(log_level);
    
    if verbose {
        info!("Verbose mode enabled");
    }
    
    if args.len() < 2 {
        eprintln!("Usage: {} [--verbose] <container-command>", args[0]);
        eprintln!("Environment variables:");
        eprintln!("  SHARED_DIR=/path/to/shared  - Shared directory (default: /tmp/container-notify)");
        eprintln!("  TIMEOUT=60                  - Timeout in seconds");
        process::exit(1);
    }

    // Get configuration from environment
    let shared_dir = env::var("SHARED_DIR")
        .unwrap_or_else(|_| "/tmp/container-notify".to_string());
    let status_file = format!("{}/container-status", shared_dir);
    let _pid_file = format!("{}/container-pid", shared_dir);
    let timeout: u64 = env::var("TIMEOUT")
        .unwrap_or_else(|_| "60".to_string())
        .parse()
        .unwrap_or(60);

    info!("Starting host waiter for container command: {}", args[1..].join(" "));
    info!("Shared directory: {}", shared_dir);
    info!("Timeout: {}s", timeout);

    // Check if we can communicate with systemd
    info!("Checking systemd notification availability");
    if env::var("NOTIFY_SOCKET").is_err() {
        info!("NOTIFY_SOCKET not set - systemd notifications may not work");
    }

    // Create shared directory with proper permissions
    info!("Creating shared directory");
    fs::create_dir_all(&shared_dir)
        .expect("Failed to create shared directory");
    
    info!("Setting directory permissions");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shared_dir)
            .expect("Failed to get directory metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shared_dir, perms)
            .expect("Failed to set directory permissions");
    }

    // Start the container command in background
    info!("Starting container process");
    let mut child = Command::new(&args[1])
        .args(&args[2..])
        .spawn()
        .expect("Failed to start container command");

    let container_pid = child.id();
    info!("Container started with PID {}", container_pid);

    // Set up cleanup handler
    let shared_dir_cleanup = shared_dir.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, cleaning up...");
        let _ = fs::remove_dir_all(&shared_dir_cleanup);
        process::exit(1);
    }).expect("Failed to set signal handler");

    // Monitor for ready signal and status updates
    info!("Waiting for container to be ready (timeout: {}s)", timeout);
    
    let start_time = Instant::now();
    let mut ready_sent = false;
    let mut last_status = String::new();
    
    loop {
        let elapsed = start_time.elapsed().as_secs();
        
        // Check timeout
        if elapsed >= timeout && !ready_sent {
            error!("Timeout waiting for container to be ready");
            let _ = child.kill();
            process::exit(1);
        }
        
        // Check if container is still running
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                info!("Container process ended");
                let exit_code = exit_status.code().unwrap_or(1);
                info!("Container exited with code {}", exit_code);
                
                // Cleanup
                let _ = fs::remove_dir_all(&shared_dir);
                process::exit(exit_code);
            }
            Ok(None) => {
                // Process is still running
                info!("Container process still running");
            }
            Err(e) => {
                error!("Failed to check container status: {}", e);
                process::exit(1);
            }
        }
        
        // Check status file for updates
        if let Ok(status) = fs::read_to_string(&status_file) {
            let status = status.trim();
            if status != last_status {
                info!("Status changed from '{}' to '{}'", last_status, status);
                
                match status {
                    "READY" => {
                        if !ready_sent {
                            info!("Container is ready! Notifying systemd...");
                            if let Err(e) = notify_ready() {
                                error!("Failed to notify systemd ready: {}", e);
                            } else {
                                ready_sent = true;
                            }
                        }
                    }
                    "STOPPING" => {
                        info!("Container is stopping...");
                        if let Err(e) = notify_stopping() {
                            error!("Failed to notify systemd stopping: {}", e);
                        }
                    }
                    "WATCHDOG" => {
                        info!("Forwarding watchdog ping");
                        if let Err(e) = notify_watchdog() {
                            error!("Failed to send watchdog ping: {}", e);
                        }
                    }
                    status if status.starts_with("STATUS:") => {
                        let status_msg = &status[7..]; // Remove "STATUS:" prefix
                        info!("Forwarding status: {}", status_msg);
                        if let Err(e) = notify_status(status_msg) {
                            error!("Failed to forward status: {}", e);
                        }
                    }
                    status if status.starts_with("MESSAGE:") => {
                        let msg = &status[8..]; // Remove "MESSAGE:" prefix
                        info!("Container message: {}", msg);
                    }
                    status if status.starts_with("EXIT:") => {
                        let exit_code_str = &status[5..]; // Remove "EXIT:" prefix
                        if let Ok(exit_code) = exit_code_str.parse::<i32>() {
                            info!("Container signaled exit with code {}", exit_code);
                            let _ = child.kill();
                            let _ = fs::remove_dir_all(&shared_dir);
                            process::exit(exit_code);
                        } else {
                            error!("Invalid exit code in status: {}", status);
                        }
                    }
                    _ => {
                        info!("Unknown status: {}", status);
                    }
                }
                last_status = status.to_string();
            }
        } else {
            info!("Status file not found yet");
        }
        
        thread::sleep(Duration::from_millis(500));
    }
}