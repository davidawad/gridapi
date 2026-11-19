//! Live tests against api.ercot.com. Ignored by default.
//!
//! Run with `ERCOT_USERNAME`, `ERCOT_PASSWORD` and `ERCOT_SUBSCRIPTION_KEY` set:
//! `cargo test --test live -- --ignored --nocapture --test-threads=1`
//!
//! NEVER run these in public CI: they use your own ERCOT credentials, which must not be shared, and ERCOT caps downloads of the same report at 3 per 12 months. Use your own credentials on your own machine.

#[path = "live/checks.rs"]
mod checks;

use gridapi_ercot::{Client, Credentials};

fn client() -> Client {
    Client::new(Credentials::from_env().expect("ERCOT_* environment variables")).expect("client")
}

macro_rules! live {
    ($($name:ident),* $(,)?) => {$(
        #[tokio::test]
        #[ignore = "needs ERCOT credentials and network"]
        async fn $name() {
            match checks::$name(&client()).await {
                Ok(summary) => println!("{}: {summary}", stringify!($name)),
                Err(failure) => panic!("{}: {failure}", stringify!($name)),
            }
        }
    )*};
}

live!(
    catalog,
    product,
    rt_spp,
    dam_spp,
    rt_lmp,
    actual_load,
    load_forecast,
    wind_5min,
    solar_5min,
    system_generation,
    fuel_mix,
    archive,
    generic_query_all,
    unknown_param,
);
