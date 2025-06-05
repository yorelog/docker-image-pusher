//! Command-line interface module

mod args;
mod runner;

pub use args::Args;
pub use runner::Runner;

use crate::error::Result;

pub fn run() -> Result<()> {
    let args = Args::parse_args();
    let runner = Runner::new(args)?;
    
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| crate::error::PusherError::Io(e))?;
    
    rt.block_on(runner.run())
}