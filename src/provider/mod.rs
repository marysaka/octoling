use once_cell::sync::Lazy;

use crate::config::{ImageConfig, GLOBAL_PROVIDER_CONFIG};
use std::collections::HashMap;
use std::sync::Mutex;

pub type Result<T> = std::result::Result<T, ProviderError>;

#[derive(Debug, PartialEq, Eq)]
pub enum ProviderError {
    InvalidImage,
    RunnerCreationFailed,
    RunnerNotFound,
    RunnerDestructionFailed,
    RunnerStartFailed,
    RunnerStopFailed,
    RunnerRunFailed,
    Unknown(String),
}

#[cfg(target_os = "linux")]
mod lxc;

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub wait: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        let mut env = HashMap::new();

        env.insert(
            String::from("PATH"),
            String::from("/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/bin:/usr/local/sbin"),
        );
        env.insert(String::from("HOME"), String::from("/"));
        env.insert(
            String::from("DEBIAN_FRONTEND"),
            String::from("noninteractive"),
        );

        RunOptions {
            cwd: String::from("/"),
            env,
            wait: true,
        }
    }
}

pub trait Runner: Send {
    fn id(&self) -> Result<String>;
    fn start(&self) -> Result<()>;
    fn run(&self, args: &[&str], options: &RunOptions) -> Result<i32>;
    fn stop(&self) -> Result<()>;
}

pub trait Provider: Send {
    fn create(&mut self, image_config: &ImageConfig, runner_id: &str) -> Result<Box<dyn Runner>>;
    fn get(&mut self, runner_id: &str) -> Result<Box<dyn Runner>>;
    fn destroy(&mut self, runner_id: &str) -> Result<()>;
}

pub static GLOBAL_PROVIDER: Lazy<HashMap<String, Mutex<Box<dyn Provider>>>> = Lazy::new(|| {
    let mut providers = HashMap::new();

    for provider_config in &*GLOBAL_PROVIDER_CONFIG {
        let provider: Box<dyn Provider> = match provider_config.provider_type.as_str() {
            "lxc" => {
                if cfg!(target_os = "linux") {
                    Box::new(lxc::LxcProvider)
                } else {
                    unimplemented!("LXC provider is only availaible on Linux");
                }
            }
            _ => unimplemented!("{}", provider_config.provider_type),
        };

        providers.insert(provider_config.id.clone(), Mutex::new(provider));
    }

    providers
});

pub fn init() {
    Lazy::force(&GLOBAL_PROVIDER);
}

pub fn get_provider(id: &str) -> Option<&'static Mutex<Box<dyn Provider>>> {
    GLOBAL_PROVIDER.get(id)
}
