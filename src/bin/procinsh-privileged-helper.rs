//! Private privileged acquisition executable. It has no HTTP listener.
#[path = "../http_server/helper_root.rs"]
mod http_server;
#[path = "../helper/privileges.rs"]
mod privileges;

fn main() -> std::process::ExitCode {
    env_logger::init();
    match http_server::helper::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("privileged helper: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
