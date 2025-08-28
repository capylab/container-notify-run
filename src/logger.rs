use log::LevelFilter;
use std::io::{self, Write};

// Simple logger implementation shared by both binaries
pub struct SimpleLogger {
    level: LevelFilter,
}

impl SimpleLogger {
    fn new(level: LevelFilter) -> Self {
        Self { level }
    }
    
    pub fn init(level: LevelFilter) {
        let logger = SimpleLogger::new(level);
        log::set_boxed_logger(Box::new(logger))
            .expect("Failed to set logger");
        log::set_max_level(level);
    }
}

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let hours = (timestamp % 86400) / 3600;
            let minutes = (timestamp % 3600) / 60;
            let seconds = timestamp % 60;
            
            eprintln!("[{} {:02}:{:02}:{:02}] {}", 
                     record.target().to_uppercase(), 
                     hours, minutes, seconds, 
                     record.args());
        }
    }

    fn flush(&self) {
        io::stderr().flush().unwrap();
    }
}