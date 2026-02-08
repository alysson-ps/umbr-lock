use std::sync::mpsc;

use clap::{Parser, Subcommand};
use uheex::parser::parser;
use umbr_core::{Anyresult, UmbrError};

/// Umbr Lock CLI - Manage and preview Umbr lock files
#[derive(Parser, Debug)]
#[command(
    name = "umbr-lock",
    author = "Alysson <dev.alysson@hotmail.com>",
    version
)]
struct UmbrLock {
    /// Watch for changes in lock files
    #[arg(short, long, default_value_t = false)]
    watch: bool,

    /// Path to the layout file
    #[arg(short, long, default_value = "layout.uheex")]
    config: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Locker {
        /// Preview the current lock file
        #[arg(short, long, default_value_t = false)]
        preview: bool,

        /// Path to the layout file
        #[arg(short, long, default_value = "layout.uheex")]
        config: String,
    },
    Parser {
        /// Path to the layout file
        #[arg(short, long, default_value = "layout.uheex")]
        config: String,

        /// Output raw parse tree
        #[arg(short, long, default_value_t = false)]
        raw: bool,
    },
}

enum RuntimeMode {
    Preview,
    Standard,
}

#[tokio::main]
async fn main() -> Anyresult<()> {
    let cli = UmbrLock::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Parser { config, raw } => {
                let config = std::fs::read_to_string(&config).unwrap();

                if let Some(ast) = parser(&config.as_str()) {
                    if raw {
                        print!("{}", ast.as_raw());
                        return Ok(());
                    }

                    print!("{}", ast.to_slint());
                }
            }
            Commands::Locker { config, preview } => {
                let mode = if preview {
                    RuntimeMode::Preview
                } else {
                    RuntimeMode::Standard
                };

                let config = std::fs::read_to_string(&config).unwrap();

                if let Some(ast) = parser(config.as_str()) {
                    match mode {
                        RuntimeMode::Preview => {
                            // let _ui_runtime = umbr_ui::mount_ui(senrder ast);
                        }
                        RuntimeMode::Standard => {
                            let (sender_to_render, receiver_from_windowing) =
                                mpsc::channel::<umbr_ui::types::WindowingMessage>();
                            let (sender_to_windowing, receiver_from_render) =
                                mpsc::channel::<umbr_ui::types::UiMessage>();

                            let mut windowing = umbr_ui::win::WindowingApp::initialize(
                                sender_to_render.clone(),
                                receiver_from_render,
                            )
                            .map_err(|err| UmbrError::Generic(err.to_string()))?;

                            windowing
                                .initial_roundtrip()
                                .map_err(|err| UmbrError::Generic(err.to_string()))?;

                            let mut ui_runtime = umbr_ui::UiRuntime::standard(
                                ast,
                                sender_to_windowing.clone(),
                                receiver_from_windowing,
                            )
                            .await?;

                            windowing
                                .process_ui_messages()
                                .map_err(|err| UmbrError::Generic(err.to_string()))?;

                            while windowing.is_running() && ui_runtime.is_running() {
                                ui_runtime.process_messages()?;

                                windowing
                                    .dispatch_blocking()
                                    .map_err(|err| UmbrError::Generic(err.to_string()))?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
