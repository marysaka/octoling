mod definition;

use super::Provider;
use super::ProviderError;
use super::Result;
use super::RunOptions;
use super::Runner;
use crate::config::ImageConfig;

use definition::*;

pub struct LxcRunner {
    container: Container,
}

impl Runner for LxcRunner {
    fn id(&self) -> Result<String> {
        if let Ok(res) = self.container.config_file_name() {
            return Ok(res);
        }

        Err(ProviderError::Unknown(String::from(
            "Cannot retrieve runer id!",
        )))
    }

    fn start(&self) -> Result<()> {
        if self.container.want_daemonize(true).is_err() {
            return Err(ProviderError::RunnerStartFailed);
        }

        if self.container.start(false, &["/sbin/init"]).is_err() {
            return Err(ProviderError::RunnerStartFailed);
        }

        Ok(())
    }

    fn stop(&self) -> Result<()> {
        if self.container.stop().is_err() {
            return Err(ProviderError::RunnerStopFailed);
        }

        Ok(())
    }

    fn run(&self, args: &[&str], options: &RunOptions) -> Result<i32> {
        if let Ok((result_code, _stdout, _stderr)) = self.container.run(args, options) {
            return Ok(result_code);
        }

        Err(ProviderError::RunnerRunFailed)
    }
}

#[derive(Debug)]
pub struct LxcProvider;

impl LxcProvider {
    fn get_container(&self, runner_id: &str) -> Result<LxcRunner> {
        if let Ok(container) = Container::new(runner_id) {
            if container.is_defined() {
                return Ok(LxcRunner { container });
            }
        }

        Err(ProviderError::RunnerNotFound)
    }
}

impl Provider for LxcProvider {
    fn destroy(&mut self, runner_id: &str) -> Result<()> {
        let runner = self.get_container(runner_id)?;

        runner.stop()?;

        if runner.container.destroy().is_err() {
            return Err(ProviderError::RunnerDestructionFailed);
        }

        Ok(())
    }

    fn create(&mut self, image_config: &ImageConfig, runner_id: &str) -> Result<Box<dyn Runner>> {
        if let Ok(mut container) = Container::new(runner_id) {
            if !container.is_defined() {
                let mut split = image_config.name.split(':');

                if let Some(template) = split.next() {
                    let args: Vec<&str> = split.collect();

                    if args.len() < 3 {
                        return Err(ProviderError::InvalidImage);
                    }

                    let argv = vec!["--dist", args[0], "--release", args[1], "--arch", args[2]];
                    let result = container.create(template, &argv[..]);

                    if result.is_ok() {
                        return Ok(Box::new(LxcRunner { container }));
                    }
                } else {
                    return Err(ProviderError::InvalidImage);
                }
            }
        }

        Err(ProviderError::RunnerCreationFailed)
    }

    fn get(&mut self, runner_id: &str) -> Result<Box<dyn Runner>> {
        // TODO: check if defined?
        Ok(Box::new(self.get_container(runner_id)?))
    }
}
