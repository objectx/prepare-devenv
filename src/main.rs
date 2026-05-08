#[cfg(not(windows))]
compile_error!("prepare-devenv targets Windows only");

mod capture;
mod cli;
mod diff;
mod discovery;
mod error;
mod runner;
mod shell;

#[allow(unused_imports)]
pub use error::{Error, Result};

fn main() {
    unimplemented!("orchestration is implemented in Task 9");
}
