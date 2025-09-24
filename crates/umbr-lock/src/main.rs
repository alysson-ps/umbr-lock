use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

    if args.preview {
        let (sender_to_render, receiver_from_windowing) =
            mpsc::channel::<umbr_ui::types::WindowingMessage>();
        let (sender_to_windowing, receiver_from_render) =
            mpsc::channel::<umbr_ui::types::UiMessage>();

        thread::spawn(move || {
            if umbr_ui::win::windowing_thread(sender_to_render.clone(), receiver_from_render)
                .is_err()
            {
                sender_to_render
                    .send(umbr_ui::types::WindowingMessage::Quit)
                    .unwrap();
            }
        });

        if let Some(_ast) = parser(SRC) {
            let _ = umbr_ui::mount_ui(sender_to_windowing.clone(), receiver_from_windowing);

            // sender_to_windowing
            //     .send(umbr_ui::types::UiMessage::Render {
            //         width: w,
            //         height: h,
            //         stride: s,
            //         pixels,
            //     })
            //     .unwrap();
        }

        // thread::sleep(Duration::from_secs(2));

        // sender_to_windowing
        //     .send(umbr_ui::types::UiMessage::UnlockWithPassword {
        //         password: "test".into(),
        //     })
        //     .unwrap();
    }

    Ok(())
}
