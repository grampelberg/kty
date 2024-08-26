use std::result::Result;

use prometheus::{Encoder, TextEncoder};
use warp::{
    reject::{self, Reject},
    Rejection, Reply,
};

#[allow(dead_code)]
#[derive(Debug)]
struct GatherError(prometheus::Error);

impl Reject for GatherError {}

pub async fn metrics() -> Result<impl Reply, Rejection> {
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&prometheus::gather(), &mut buffer)
        .map_err(|err| reject::custom(GatherError(err)))?;

    Ok(buffer)
}
