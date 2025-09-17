use clap::Parser;
use uheex::parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    name: String,

    /// Number of times to greet
    #[arg(short, long, default_value_t = 1)]
    count: u8,
}

fn main() {
    // let _ = umbr_core::lock();
    // parser(r#"<U.Button text="Desbloquear" u-click={@unlock_user} /> "#);
    parser(r#"<U.Button :text="Desbloquear"> <U.label> <% @hello %> </U.label> </U.Button>"#);
}
