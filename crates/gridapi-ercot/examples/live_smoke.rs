//! Runs every live check against api.ercot.com and prints sample values.
//!
//! ```text
//! ERCOT_USERNAME=... ERCOT_PASSWORD=... ERCOT_SUBSCRIPTION_KEY=... \
//!     cargo run --example live_smoke
//! ```
//!
//! Exits non-zero when any check fails.
//!
//! NEVER run these in public CI: they use your own ERCOT credentials, which must not be shared, and ERCOT caps downloads of the same report at 3 per 12 months. Use your own credentials on your own machine.

#[path = "../tests/live/checks.rs"]
mod checks;

use std::process::ExitCode;

use gridapi_ercot::{Client, Credentials};

#[tokio::main]
async fn main() -> ExitCode {
    let client = match Credentials::from_env().and_then(Client::new) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("cannot start: {e}");
            return ExitCode::from(2);
        }
    };
    let mut failures = 0u32;
    for (name, outcome) in checks::all(&client).await {
        match outcome {
            Ok(summary) => println!("PASS {name}: {summary}"),
            Err(failure) => {
                failures += 1;
                println!("FAIL {name}: {failure}");
            }
        }
    }
    println!("{failures} failed");
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
