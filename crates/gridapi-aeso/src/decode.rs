//! Pure decoders: response text in, typed values out. [`crate::Client`] uses
//! these, and you can too when you already hold a response body (a cache, a
//! recorded fixture, a different HTTP stack).

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::model::assets::{Asset, PoolParticipant};
use crate::model::csd::{GenerationAssets, SupplyDemandSummary};
use crate::model::interchange::{InterchangeReport, IntertieOutage, OutagesPayload};
use crate::model::market::{
    MeritOrderHour, MeritOrderPayload, MeteredParticipant, ReserveHour, ReservePayload,
    UnitCommitment, UnitCommitmentPayload,
};
use crate::model::price::{
    ActualForecastReport, CurrentSystemMarginalPrice, InternalLoad, PoolPrice, PoolPriceReport,
    SmpReport, SystemMarginalPrice,
};
use crate::model::supply::{FuelCapacity, LoadOutageHour, LoadOutagePayload};
use crate::model::Reply;

fn reply<T: DeserializeOwned>(endpoint: &str, body: &str) -> Result<T> {
    serde_json::from_str::<Reply<T>>(body)
        .map(|r| r.payload)
        .map_err(|source| Error::Decode {
            endpoint: endpoint.to_owned(),
            source,
        })
}

/// Decode `poolprice-api/v1.1/price/poolPrice`.
pub fn pool_price(body: &str) -> Result<Vec<PoolPrice>> {
    reply::<PoolPriceReport>("poolPrice", body).map(|r| r.rows)
}

/// Decode `systemmarginalprice-api/v1.1/price/systemMarginalPrice`.
pub fn system_marginal_price(body: &str) -> Result<Vec<SystemMarginalPrice>> {
    reply::<SmpReport<SystemMarginalPrice>>("systemMarginalPrice", body).map(|r| r.rows)
}

/// Decode `.../systemMarginalPrice/current`.
pub fn current_system_marginal_price(body: &str) -> Result<Vec<CurrentSystemMarginalPrice>> {
    reply::<SmpReport<CurrentSystemMarginalPrice>>("systemMarginalPrice/current", body)
        .map(|r| r.rows)
}

/// Decode `actualforecast-api/v1/load/albertaInternalLoad`.
pub fn internal_load(body: &str) -> Result<Vec<InternalLoad>> {
    reply::<ActualForecastReport>("albertaInternalLoad", body).map(|r| r.rows)
}

/// Decode `currentsupplydemand-api/v2/csd/summary/current`.
pub fn supply_demand_summary(body: &str) -> Result<SupplyDemandSummary> {
    reply("csd/summary/current", body)
}

/// Decode `currentsupplydemand-api/v1/csd/generation/assets/current`.
pub fn generation_assets(body: &str) -> Result<GenerationAssets> {
    reply("csd/generation/assets/current", body)
}

/// Decode `itc/v1/interchange`. An empty `return: {}` yields an empty report.
pub fn interchange(body: &str) -> Result<InterchangeReport> {
    reply("interchange", body)
}

/// Decode `itc/v1/outage`.
pub fn intertie_outages(body: &str) -> Result<Vec<IntertieOutage>> {
    reply::<OutagesPayload>("outage", body).map(|p| p.outages.outage)
}

/// Decode `energymeritorder-api/v1/meritOrder/energy`.
pub fn merit_order(body: &str) -> Result<Vec<MeritOrderHour>> {
    reply::<MeritOrderPayload>("meritOrder/energy", body).map(|p| p.data)
}

/// Decode `operatingreserveoffercontrol-api/v1/operatingReserveOfferControl`.
pub fn operating_reserve_offers(body: &str) -> Result<Vec<ReserveHour>> {
    reply::<ReservePayload>("operatingReserveOfferControl", body).map(|p| p.hours)
}

/// Decode `meteredvolume-api/v1/meteredvolume/details`.
pub fn metered_volume(body: &str) -> Result<Vec<MeteredParticipant>> {
    reply("meteredvolume/details", body)
}

/// Decode `unitcommitmentdata-api/v2/unitCommitment`.
pub fn unit_commitment(body: &str) -> Result<Vec<UnitCommitment>> {
    reply::<UnitCommitmentPayload>("unitCommitment", body).map(|p| p.unit_commitment)
}

/// Decode `aiesgencapacity-api/v1/AIESGenCapacity`.
pub fn aies_gen_capacity(body: &str) -> Result<Vec<FuelCapacity>> {
    reply("AIESGenCapacity", body)
}

/// Decode `loadoutageforecast-api/v1/loadOutageReport`.
pub fn load_outage_forecast(body: &str) -> Result<Vec<LoadOutageHour>> {
    reply::<LoadOutagePayload>("loadOutageReport", body).map(|p| p.hours)
}

/// Decode `assetlist-api/v1/assetlist`.
pub fn asset_list(body: &str) -> Result<Vec<Asset>> {
    reply("assetlist", body)
}

/// Decode `poolparticipant-api/v1/poolparticipantlist`.
pub fn pool_participants(body: &str) -> Result<Vec<PoolParticipant>> {
    reply("poolparticipantlist", body)
}

/// The JSON error body AESO's backend returns on HTTP 400:
/// `{"timestamp": "...", "message": "...", "details": "uri=..."}`.
pub(crate) fn api_error(status: u16, body: &str) -> Option<Error> {
    #[derive(serde::Deserialize)]
    struct Body {
        message: String,
        #[serde(default)]
        details: Option<String>,
    }
    serde_json::from_str::<Body>(body).ok().map(|b| Error::Api {
        status,
        message: b.message,
        details: b.details,
    })
}
