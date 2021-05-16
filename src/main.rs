extern crate lts;

fn main() {
    match lts::cli_run() {
        Ok(()) => {},
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
