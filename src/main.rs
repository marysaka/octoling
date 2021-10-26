#![allow(dead_code)]

mod api;
mod config;
mod manager;
mod provider;
mod utils;

use api::api_routes;
//use api::github_connector_routes;
use api::github_webhook_routes;

use warp::Filter;

#[tokio::main]
async fn main() {
    config::load();
    provider::init();

    let routes = api_routes().or(github_webhook_routes());

    warp::serve(routes).run(([127, 0, 0, 1], 8000)).await;
}
