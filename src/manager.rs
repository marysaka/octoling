use crate::config::{GithubConfig, ImageConfig};
use crate::provider::GLOBAL_PROVIDER;
use crate::provider::{self, ProviderError, RunOptions, Runner};

use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum ManagerError {
    ProviderNotFound,
    Provider(ProviderError),
    TokenRequestFailed,
    InstallationFailed,
}

impl From<ProviderError> for ManagerError {
    fn from(err: ProviderError) -> ManagerError {
        ManagerError::Provider(err)
    }
}

pub type Result<T> = std::result::Result<T, ManagerError>;

// TODO: https://docs.github.com/en/rest/reference/actions#list-runner-applications-for-a-repository
const RUNNER_DL_URL: &str = "https://github.com/actions/runner/releases/download/v2.283.3/actions-runner-linux-x64-2.283.3.tar.gz";

fn ensure_success_error_code(error_code: i32) -> Result<()> {
    if error_code != 0 {
        Err(ManagerError::InstallationFailed)
    } else {
        Ok(())
    }
}

fn setup_runner(
    runner: &Mutex<Box<dyn Runner>>,
    label: &str,
    registration_token: &str,
    repository_url: &str,
    runner_id: &str,
) -> Result<()> {
    let mut options = RunOptions::default();

    let runner = runner.lock().unwrap();

    ensure_success_error_code(runner.run(&["apt-get", "update"], &options)?)?;
    ensure_success_error_code(runner.run(
        &["apt-get", "install", "-y", "curl", "tar", "gzip", "sudo"],
        &options,
    )?)?;
    ensure_success_error_code(runner.run(
        &["curl", "https://get.docker.com/", "-o", "install_docker.sh"],
        &options,
    )?)?;
    ensure_success_error_code(
        runner.run(&["sh", "install_docker.sh", "install", "runner"], &options)?,
    )?;

    ensure_success_error_code(runner.run(
        &["curl", "-L", RUNNER_DL_URL, "-o", "runner.tar.gz"],
        &options,
    )?)?;
    ensure_success_error_code(runner.run(&["useradd", "-m", "runner"], &options)?)?;
    ensure_success_error_code(runner.run(
        &[
            "bash",
            "-c",
            "echo",
            "runner ALL=(ALL:ALL) NOPASSWD:ALL",
            ">>",
            "/etc/sudoers",
        ],
        &options,
    )?)?;
    ensure_success_error_code(runner.run(&["usermod", "-a", "-G", "docker", "runner"], &options)?)?;
    ensure_success_error_code(runner.run(&["mkdir", "/runner"], &options)?)?;
    ensure_success_error_code(runner.run(&["chown", "runner:runner", "/runner"], &options)?)?;
    ensure_success_error_code(runner.run(
        &[
            "sudo",
            "-u",
            "runner",
            "tar",
            "xzf",
            "runner.tar.gz",
            "-C",
            "/runner",
        ],
        &options,
    )?)?;

    options.cwd = String::from("/runner");

    let mut labels = String::from("octoling");
    labels.push(',');
    labels.push_str(label);

    // https://docs.github.com/en/rest/reference/actions#create-a-registration-token-for-a-repository
    // https://github.com/github/platform-samples/blob/master/api/bash/migrate-repos-in-org.sh#L126
    // reqwest
    ensure_success_error_code(runner.run(
        &[
            "sudo",
            "-u",
            "runner",
            "bash",
            "config.sh",
            "--unattended",
            "--ephemeral",
            "--url",
            repository_url,
            "--token",
            registration_token,
            "--name",
            // Do not trust OS naming
            runner_id,
            "--labels",
            labels.as_str(),
        ],
        &options,
    )?)?;

    ensure_success_error_code(runner.run(&["bash", "svc.sh", "install", "runner"], &options)?)?;
    ensure_success_error_code(runner.run(&["bash", "svc.sh", "start"], &options)?)?;
    Ok(())
}

pub async fn start_new_clean_runner(
    image_config: &ImageConfig,
    runner_id: &str,
) -> Result<Box<dyn Runner>> {
    if let Some(provider) = provider::get_provider(image_config.provider_id.as_str()) {
        let mut provider = provider.lock().unwrap();

        let runner = provider.create(image_config, runner_id)?;

        if let Err(startup_error) = runner.start() {
            // Ensure that we destroy on startup error.
            let _ = provider.destroy(runner_id);

            // Return original startup error
            return Err(ManagerError::from(startup_error));
        }

        return Ok(runner);
    }

    Err(ManagerError::ProviderNotFound)
}

pub async fn destroy_runner_with_runner_id(runner_id: &str) -> Result<()> {
    for provider_id in GLOBAL_PROVIDER.keys() {
        let result = destroy_runner(provider_id.as_str(), runner_id).await;

        match result {
            Ok(_) => return Ok(()),
            Err(error) => {
                if error != ManagerError::Provider(ProviderError::RunnerNotFound) {
                    return Err(error);
                }
            }
        }
    }

    Err(ManagerError::Provider(ProviderError::RunnerNotFound))
}

pub async fn destroy_runner(provider_id: &str, runner_id: &str) -> Result<()> {
    if let Some(provider) = provider::get_provider(provider_id) {
        let mut provider = provider.lock().unwrap();

        provider.destroy(runner_id)?;

        Ok(())
    } else {
        Err(ManagerError::ProviderNotFound)
    }
}

pub async fn start_new_runner(
    image_config: &ImageConfig,
    github_config: GithubConfig,
    label: &str,
    runner_id: &str,
) -> Result<Mutex<Box<dyn Runner>>> {
    let runner_token = github_config
        .request_new_repo_runner_token()
        .await
        .ok_or(ManagerError::TokenRequestFailed)?;
    let repository_url = github_config.get_repo_url();
    let runner = Mutex::new(start_new_clean_runner(image_config, runner_id).await?);

    // FIXME: find a better way to know when the network is ready.
    // TODO: Also move to Runner::start?
    std::thread::sleep(Duration::from_secs(5));

    if let Err(error) = setup_runner(
        &runner,
        label,
        runner_token.as_str(),
        repository_url.as_str(),
        runner_id,
    ) {
        let _ = runner.lock().unwrap().stop();
        let _ = destroy_runner(image_config.provider_id.as_str(), runner_id).await;

        return Err(error);
    }

    Ok(runner)
}
