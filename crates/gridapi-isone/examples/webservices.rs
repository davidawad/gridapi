//! Print the latest Internal Hub price from the authenticated Web Services API.
//!
//! ```text
//! ISONE_USERNAME=you ISONE_PASSWORD=secret cargo run -p gridapi-isone --example webservices
//! ```

use std::process::ExitCode;

use gridapi_isone::{
    express::Revision,
    webservices::{Scope, WebServices, Window},
    Error, INTERNAL_HUB_ID,
};

async fn run() -> Result<(), Error> {
    let ws = WebServices::from_env()?;
    let rows = ws
        .five_minute_lmp(
            Revision::Preliminary,
            Window::Current,
            Scope::Location(INTERNAL_HUB_ID),
        )
        .await?;
    for row in rows {
        println!("{row:?}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
