//! Private privileged acquisition executable. It has no HTTP listener.
//!
//! # Interface
//! No items are exported. Startup delegates to [`privileged_helper::launch_privileged_helper`].
#[cfg(test)]
#[path = "test_http/presentation.rs"]
mod http_server;
#[path = "../ipc/root.rs"]
mod ipc;
#[path = "root.rs"]
mod privileged_helper;
#[cfg(test)]
#[path = "../test_support.rs"]
mod test_support;

fn main() -> std::process::ExitCode {
    env_logger::init();
    match privileged_helper::launch_privileged_helper() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("privileged helper: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
