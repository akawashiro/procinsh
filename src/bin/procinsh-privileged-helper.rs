//! Private privileged acquisition executable. It has no HTTP listener.
#[path = "../privileged_helper/root.rs"]
mod privileged_helper;

fn main() -> std::process::ExitCode {
    env_logger::init();
    match privileged_helper::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("privileged helper: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
