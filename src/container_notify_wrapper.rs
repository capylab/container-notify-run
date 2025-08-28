use distrobox_run_notify::SimpleLogger;
use log::{error, info, LevelFilter};
use std::env;
use std::fs;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::os::unix::process::CommandExt;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

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
        eprintln!("Usage: {} [--verbose] <command> [args...]", args[0]);
        eprintln!("Options:");
        eprintln!("  --verbose    Show detailed logging");
        eprintln!("Environment variables:");
        eprintln!("  SHARED_DIR=/shared  - Shared directory for communication");
        process::exit(1);
    }

    let shared_dir = env::var("SHARED_DIR").unwrap_or_else(|_| "/shared".to_string());
    let socket_path = format!("{}/notify.sock", shared_dir);
    let status_file = format!("{}/container-status", shared_dir);
    let pid_file = format!("{}/container-pid", shared_dir);

    info!("Starting container wrapper for: {}", args[1]);
    info!("Shared directory: {}", shared_dir);
    info!("Socket path: {}", socket_path);

    // Create shared directory
    info!("Creating shared directory");
    fs::create_dir_all(&shared_dir)
        .expect("Failed to create shared directory");
    
    // Write PID and status
    info!("Writing PID {} to {}", process::id(), pid_file);
    fs::write(&pid_file, process::id().to_string())
        .expect("Failed to write PID file");
        
    info!("Writing initial status");
    fs::write(&status_file, "STARTING")
        .expect("Failed to write initial status");

    // Remove old socket
    info!("Removing old socket if present");
    let _ = fs::remove_file(&socket_path);

    // Create socket
    info!("Creating Unix socket at {}", socket_path);
    let socket = UnixDatagram::bind(&socket_path)
        .expect("Failed to create Unix socket");

    // Fork using unsafe libc call
    info!("Forking socket proxy process");
    let pid = unsafe { libc::fork() };
    
    match pid {
        -1 => {
            error!("Fork failed");
            process::exit(1);
        }
        0 => {
            // Child - run proxy
            info!("Starting socket proxy process");
            if let Err(e) = run_socket_proxy(socket, status_file) {
                error!("Socket proxy failed: {}", e);
                process::exit(1);
            }
            process::exit(0);
        }
        _ => {
            // Parent continues
            info!("Socket proxy started with PID {}", pid);
            
            // Wait for socket to be ready
            info!("Waiting for socket to be ready");
            thread::sleep(Duration::from_millis(200));
            
            // Verify socket exists
            info!("Verifying socket exists");
            if !std::path::Path::new(&socket_path).exists() {
                error!("Socket was not created at {}", socket_path);
                process::exit(1);
            }
            
            // Set environment variable
            info!("Setting NOTIFY_SOCKET environment variable");
            unsafe {
                env::set_var("NOTIFY_SOCKET", &socket_path);
            }
            info!("Starting main process with NOTIFY_SOCKET={}", socket_path);
            
            // Exec target
            info!("Executing target command: {}", args[1]);
            let error = Command::new(&args[1]).args(&args[2..]).exec();
            error!("Failed to exec {}: {}", args[1], error);
            process::exit(1);
        }
    }
}

fn run_socket_proxy(socket: UnixDatagram, status_file: String) -> io::Result<()> {
    info!("Socket proxy listening...");
    let mut buffer = [0u8; 4096];
    
    loop {
        match socket.recv(&mut buffer) {
            Ok(size) => {
                let message = String::from_utf8_lossy(&buffer[..size]);
                info!("Raw message received: {:?}", message);
                if let Err(e) = process_message(&message, &status_file) {
                    error!("Failed to process message: {}", e);
                }
            }
            Err(e) => {
                error!("Socket error: {}", e);
                break;
            }
        }
    }
    Ok(())
}

fn process_message(message: &str, status_file: &str) -> io::Result<()> {
    info!("Processing message: {:?}", message);
    
    for line in message.lines() {
        let line = line.trim();
        if line.is_empty() { 
            info!("Skipping empty line");
            continue; 
        }
        
        info!("Processing line: {}", line);
        
        let content = match line {
            "READY=1" => {
                info!("Process signaled ready!");
                "READY"
            },
            "STOPPING=1" => {
                info!("Process is stopping");
                "STOPPING"
            },
            "WATCHDOG=1" => {
                info!("Watchdog ping received");
                "WATCHDOG"
            },
            _ if line.starts_with("STATUS=") => {
                info!("Status update: {}", line);
                info!("Writing to status file: STATUS:{}", line);
                return fs::write(status_file, format!("STATUS:{}", line));
            }
            _ => {
                info!("Other message: {}", line);
                info!("Writing to status file: MESSAGE:{}", line);
                return fs::write(status_file, format!("MESSAGE:{}", line));
            }
        };
        
        info!("Writing to status file: {}", content);
        fs::write(status_file, content)?;
    }
    Ok(())
}