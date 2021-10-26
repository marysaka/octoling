use hex::FromHex;
use hmac::{
    digest::{consts::U32, generic_array::GenericArray},
    Hmac, Mac, NewMac,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::convert::Infallible;
use warp::{http::StatusCode, Filter};

use crate::config::{self, GLOBAL_GITHUB_CONFIG, SHA256_SIZE};
use crate::manager;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub login: String,
    pub id: u64,
    pub node_id: String,
    pub avatar_url: String,
    pub gravatar_id: String,
    pub url: String,
    pub html_url: String,
    pub followers_url: String,
    pub following_url: String,
    pub gists_url: String,
    pub starred_url: String,
    pub subscriptions_url: String,
    pub organizations_url: String,
    pub repos_url: String,
    pub events_url: String,
    pub received_events_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Repository {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub owner: User,
    pub html_url: String,
    pub description: String,
    pub fork: bool,
    pub url: String,
    pub visibility: String,
    pub default_branch: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowJob {
    pub id: u64,
    pub run_id: u64,
    pub run_attempt: u64,
    pub node_id: String,
    pub head_sha: String,
    pub url: String,
    pub html_url: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub name: String,
    pub labels: Vec<String>,
    pub runner_id: Option<u64>,
    pub runner_name: Option<String>,
    pub runner_group_id: Option<u64>,
    pub runner_group_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowJobEvent {
    pub repository: Repository,
    pub sender: User,
    pub workflow_job: WorkflowJob,
}

const SHA256_HEX_SIZE: usize = 64;
const SHA256_PREFIX: &str = "sha256=";

fn get_runner_id_by_job_event(event: &WorkflowJobEvent) -> String {
    format!(
        "octoling-{}-{}-{}",
        event.repository.owner.login.as_str(),
        event.repository.name.as_str(),
        event.workflow_job.id
    )
}

fn format_workflow_job_prefix(event: &WorkflowJobEvent) -> String {
    format!(
        "octoling: Job #{} ({}/{}):",
        event.workflow_job.id,
        event.repository.owner.login.as_str(),
        event.repository.name.as_str()
    )
}

async fn handle_workflow_job_queued(event: WorkflowJobEvent) {
    let log_prefix = format_workflow_job_prefix(&event);
    println!("{} queued", log_prefix);

    let github_config = config::get_github_config_by_owner_and_repo(
        event.repository.owner.login.as_str(),
        event.repository.name.as_str(),
    );

    if let Some(github_config) = github_config {
        for label in &event.workflow_job.labels {
            if let Some(image_config) = config::get_image_config_by_label(label.as_str()) {
                let runner_id = get_runner_id_by_job_event(&event);

                println!("{} Creating and starting runner {}", log_prefix, runner_id);

                let result = manager::start_new_runner(
                    &image_config,
                    github_config,
                    image_config.labels[0].as_str(),
                    runner_id.as_str(),
                )
                .await;

                match result {
                    Ok(_) => println!("{} Started runner {}", log_prefix, runner_id),
                    Err(error) => eprintln!(
                        "{} Cannot start runner {} for label \"{}\": {:?}",
                        log_prefix, runner_id, label, error
                    ),
                }

                return;
            }
        }
    }

    println!("{} cannot be handled by this instance.", log_prefix);
}

async fn handle_workflow_job_completed(event: WorkflowJobEvent) {
    let log_prefix = format_workflow_job_prefix(&event);

    println!("{} completed", log_prefix);

    if let Some(runner_id) = &event.workflow_job.runner_name {
        match manager::destroy_runner_with_runner_id(runner_id).await {
            Ok(()) => {
                println!("{} {} was destroyed", log_prefix, runner_id);
            }
            Err(error) => eprintln!(
                "{} Cannot destroy runner {}: {:?}",
                log_prefix, runner_id, error
            ),
        }
    } else {
        println!("{} cannot be handled by this instance.", log_prefix);
    }
}

async fn webhook_workflow_job_handler(event_raw: &str) -> Result<StatusCode, Infallible> {
    let event_parsing_result = serde_json::from_str(event_raw);

    if event_parsing_result.is_err() {
        return Ok(StatusCode::BAD_REQUEST);
    }

    let event: WorkflowJobEvent = event_parsing_result.unwrap();

    match event.workflow_job.status.as_str() {
        "queued" => {
            tokio::spawn(async move {
                handle_workflow_job_queued(event).await;
            });
        }
        "completed" => {
            tokio::spawn(async move {
                handle_workflow_job_completed(event).await;
            });
        }
        _ => {}
    }

    Ok(StatusCode::OK)
}

async fn webhook_handler(
    event_type: String,
    signature: String,
    data: bytes::Bytes,
) -> Result<impl warp::Reply, Infallible> {
    if signature.len() != SHA256_HEX_SIZE + SHA256_PREFIX.len()
        || !signature.starts_with(SHA256_PREFIX)
    {
        return Ok(StatusCode::BAD_REQUEST);
    }

    let provided_hex_signature =
        <[u8; SHA256_SIZE]>::from_hex(&signature[SHA256_PREFIX.len()..]).unwrap();
    let expected_signature = GenericArray::<u8, U32>::from(provided_hex_signature);

    for github_config in GLOBAL_GITHUB_CONFIG.clone() {
        let mut hasher =
            HmacSha256::new_from_slice(github_config.get_webhook_secret_slice()).unwrap();

        hasher.update(&data);

        let computed_signature = hasher.finalize().into_bytes();

        if computed_signature == expected_signature {
            return match std::str::from_utf8(data.as_ref()) {
                Ok(event_raw) => {
                    if event_type == "workflow_job" {
                        webhook_workflow_job_handler(event_raw).await
                    } else {
                        Ok(StatusCode::OK)
                    }
                }
                Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR),
            };
        }
    }

    Ok(StatusCode::UNAUTHORIZED)
}

pub fn webhook_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
{
    warp::path!("_github" / "hook")
        .and(warp::post())
        .and(warp::header::header("X-GitHub-Event"))
        .and(warp::header::header("X-Hub-Signature-256"))
        .and(warp::body::bytes())
        .and_then(webhook_handler)
}
