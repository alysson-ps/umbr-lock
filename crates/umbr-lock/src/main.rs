use std::sync::mpsc;

use clap::Parser;
use uheex::parser::parser;
use umbr_core::{Anyresult, UmbrError};

/// Umbr Lock CLI - Manage and preview Umbr lock files
#[derive(Parser, Debug)]
#[command(
    name = "umbr-lock",
    author = "Alysson <dev.alysson@hotmail.com>",
    version,
    about = "A CLI tool to manage and preview Umbr lock files.",
    long_about = "Umbr Lock is a command-line tool for managing and previewing lock files in Umbr projects."
)]
struct Args {
    /// Watch for changes in lock files
    #[arg(short, long, default_value_t = false)]
    watch: bool,

    /// Preview the current lock file
    #[arg(short, long, default_value_t = false)]
    preview: bool,
}

const SRC: &str = r#"
    <U.Box :direction "column" :spacing 8>
        <U.Button :text "Desbloquear" />
        <U.Label>
            <% @hello %>
        </U.Label>
    </U.Box>
"#;

fn main() -> Anyresult<()> {
    let args = Args::parse();

    let (sender_to_render, receiver_from_windowing) =
        mpsc::channel::<umbr_ui::types::WindowingMessage>();
    let (sender_to_windowing, receiver_from_render) = mpsc::channel::<umbr_ui::types::UiMessage>();

    let mut windowing = umbr_ui::win::WindowingApp::initialize(
        sender_to_render.clone(),
        receiver_from_render,
        args.preview,
    )
    .map_err(|err| UmbrError::Generic(err.to_string()))?;

    windowing
        .initial_roundtrip()
        .map_err(|err| UmbrError::Generic(err.to_string()))?;

    if let Some(_ast) = parser(SRC) {
        let mut ui_runtime =
            umbr_ui::mount_ui(sender_to_windowing.clone(), receiver_from_windowing)?;

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

    // thread::sleep(Duration::from_secs(2));

    // sender_to_windowing
    //     .send(umbr_ui::types::UiMessage::UnlockWithPassword {
    //         password: "test".into(),
    //     })
    //     .unwrap();

    Ok(())
}
