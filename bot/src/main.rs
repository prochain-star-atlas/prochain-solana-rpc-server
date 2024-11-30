mod utils;
mod oracles;
mod bot;
mod errors;
mod metrics;
mod rpc;
mod solana_state;
mod http;
mod model;
mod cron;

use std::panic;
use fern::Dispatch;
use chrono::Local;
use log::LevelFilter;
use fern::colors::{Color, ColoredLevelConfig};
use colored::Colorize;
use crate::bot::bot_start::start;
use crate::metrics::add_metrics_exporter;


#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    panic::set_hook(Box::new(|panic_info| {
        log::error!("Bot panicked: {:?}", panic_info);
    }));

    setup_logging().expect("Failed to setup logging.");

    add_metrics_exporter().await;

    // start the bot
    start().await;

    Ok(())
}


fn setup_logging() -> Result<(), fern::InitError> {
    // Configure colors for different log levels
    let console_colors = ColoredLevelConfig::new()
        .info(Color::Green)
        .warn(Color::Yellow)
        .error(Color::Red)
        .debug(Color::White)
        .trace(Color::BrightBlack);

// Console logging for Info and Warn, excluding Error
let console = Dispatch::new()
    .format(move |out, message, record| {
        let colored_message  = match record.level() {
            log::Level::Info | log::Level::Warn | log::Level::Error => format!("{}", message).bold().to_string(),
            _ => format!("{}", message),
        };

        out.finish(format_args!(
            "{}[{}] {}",
            Local::now().format("[%Y-%m-%d %H:%M:%S.%f]"),
            console_colors.color(record.level()),
            colored_message 
        ))
    })
    .filter(|metadata| {
        // Only allow Info and Warn levels to be logged to console
        metadata.level() == log::Level::Info || metadata.level() == log::Level::Warn || metadata.level() == log::Level::Error
    })
    .chain(std::io::stdout());



    // File logging for Info and Warn
    let file = Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                Local::now().format("[%Y-%m-%d %H:%M:%S.%f]"),
                record.level(),
                message
            ))
        })
        .filter(|metadata| {
            metadata.level() == log::Level::Info || metadata.level() == log::Level::Warn
        })
        .chain(fern::log_file("output.log")?);

    // Error file logging
    let error_file = Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}",
                Local::now().format("[%Y-%m-%d %H:%M:%S.%f]"),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Error) 
        .chain(fern::log_file("errors.log")?);

    
    Dispatch::new()
        .chain(console) 
        .chain(file)    
        .chain(error_file) 
        .apply()?;

    Ok(())
}