use clap::Parser;

fn main() {
    if let Err(error) = helixir_client::cli::Cli::parse().run() {
        eprintln!("helixir-client: {error:#}");
        std::process::exit(1);
    }
}
