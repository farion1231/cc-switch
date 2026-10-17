use clap::Parser;

fn main() {
    let cli = cc_switch_lib::cli::Cli::parse();
    match cc_switch_lib::cli::execute(cli) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("cc-switch-cli: {error}");
            std::process::exit(1);
        }
    }
}
