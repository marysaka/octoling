mod github;

use serde::{Deserialize, Serialize};
use warp::Filter;

use crate::config::SERVER_VERSION;

// TODO:
//pub use github::routes as github_connector_routes;
pub use github::webhook_routes as github_webhook_routes;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ApiVersionResponse {
    pub server_version: String,
    pub api_versions: Vec<String>,
}

fn api_version_handler() -> impl warp::Reply {
    warp::reply::json(&ApiVersionResponse {
        server_version: String::from(SERVER_VERSION),
        api_versions: vec![String::from("v0")],
    })
}

fn api_version_route() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("api" / "version")
        .and(warp::get())
        .map(api_version_handler)
}

pub fn api_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    api_version_route()
}
