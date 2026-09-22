//! Safe, digest-pinned deployment primitives for an optional VPS agent.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use registry_core::{Digest, RepositoryName};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeploymentSpec {
    pub registry: String,
    pub repository: String,
    pub tag: String,
    pub compose_project: String,
    pub compose_service: String,
    pub working_directory: PathBuf,
    pub robot_username: String,
    #[serde(skip_serializing)]
    pub robot_secret: String,
    #[serde(default = "default_health_timeout")]
    pub health_timeout_seconds: u64,
}

impl DeploymentSpec {
    pub fn validate(&self) -> Result<(), AgentError> {
        if !(self.registry.starts_with("https://") || self.registry.starts_with("http://")) {
            return Err(AgentError::InvalidConfig("registry must be an http(s) URL"));
        }
        RepositoryName::parse(&self.repository)
            .map_err(|_| AgentError::InvalidConfig("repository is invalid"))?;
        validate_component(&self.tag, "tag")?;
        validate_component(&self.compose_project, "compose project")?;
        validate_component(&self.compose_service, "compose service")?;
        if self.robot_username.trim().is_empty() || self.robot_secret.trim().is_empty() {
            return Err(AgentError::InvalidConfig(
                "pull-only robot credentials are required",
            ));
        }
        if self.health_timeout_seconds == 0 || self.health_timeout_seconds > 15 * 60 {
            return Err(AgentError::InvalidConfig("health timeout is out of range"));
        }
        Ok(())
    }

    pub fn image(&self) -> String {
        format!(
            "{}/{}",
            self.registry.trim_end_matches('/'),
            self.repository
        )
    }

    pub fn tagged_image(&self) -> String {
        format!("{}:{}", self.image(), self.tag)
    }
}

fn default_health_timeout() -> u64 {
    90
}

fn validate_component(value: &str, label: &'static str) -> Result<(), AgentError> {
    if value.is_empty()
        || value.len() > 128
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"._-".contains(&byte)))
    {
        return Err(AgentError::InvalidConfig(label));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("invalid agent configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("command failed: {program}: {stderr}")]
    CommandFailed { program: String, stderr: String },
    #[error("deployment health check failed")]
    HealthCheckFailed,
    #[error("rollback failed after an unhealthy rollout: {0}")]
    RollbackFailed(String),
    #[error("manifest source failed: {0}")]
    Source(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Result<CommandOutput, AgentError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Result<CommandOutput, AgentError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|error| AgentError::CommandFailed {
            program: program.to_owned(),
            stderr: error.to_string(),
        })?;
        if let Some(input) = stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            pipe.write_all(input)
                .await
                .map_err(|error| AgentError::CommandFailed {
                    program: program.to_owned(),
                    stderr: error.to_string(),
                })?;
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|error| AgentError::CommandFailed {
                program: program.to_owned(),
                stderr: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(AgentError::CommandFailed {
                program: program.to_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[async_trait]
pub trait DeploymentDriver: Send + Sync {
    async fn pull_digest(&self, spec: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError>;
    async fn pin_digest(&self, spec: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError>;
    async fn restart(&self, spec: &DeploymentSpec) -> Result<(), AgentError>;
    async fn healthy(&self, spec: &DeploymentSpec) -> Result<bool, AgentError>;
}

pub struct DockerComposeDriver<R> {
    runner: Arc<R>,
}

impl<R> DockerComposeDriver<R> {
    pub fn new(runner: Arc<R>) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl<R: CommandRunner + 'static> DeploymentDriver for DockerComposeDriver<R> {
    async fn pull_digest(&self, spec: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError> {
        spec.validate()?;
        self.runner
            .run(
                "docker",
                &[
                    "login".to_owned(),
                    spec.registry.clone(),
                    "--username".to_owned(),
                    spec.robot_username.clone(),
                    "--password-stdin".to_owned(),
                ],
                &spec.working_directory,
                Some(spec.robot_secret.as_bytes()),
            )
            .await?;
        self.runner
            .run(
                "docker",
                &["pull".to_owned(), format!("{}@{digest}", spec.image())],
                &spec.working_directory,
                None,
            )
            .await
            .map(|_| ())
    }

    async fn pin_digest(&self, spec: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError> {
        spec.validate()?;
        self.runner
            .run(
                "docker",
                &[
                    "image".to_owned(),
                    "tag".to_owned(),
                    format!("{}@{digest}", spec.image()),
                    spec.tagged_image(),
                ],
                &spec.working_directory,
                None,
            )
            .await
            .map(|_| ())
    }

    async fn restart(&self, spec: &DeploymentSpec) -> Result<(), AgentError> {
        spec.validate()?;
        self.runner
            .run(
                "docker",
                &[
                    "compose".to_owned(),
                    "-p".to_owned(),
                    spec.compose_project.clone(),
                    "up".to_owned(),
                    "-d".to_owned(),
                    "--no-deps".to_owned(),
                    spec.compose_service.clone(),
                ],
                &spec.working_directory,
                None,
            )
            .await
            .map(|_| ())
    }

    async fn healthy(&self, spec: &DeploymentSpec) -> Result<bool, AgentError> {
        spec.validate()?;
        let output = self
            .runner
            .run(
                "docker",
                &[
                    "inspect".to_owned(),
                    "--format={{.State.Health.Status}}".to_owned(),
                    spec.compose_service.clone(),
                ],
                &spec.working_directory,
                None,
            )
            .await?;
        Ok(output.stdout.trim() == "healthy")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RolloutResult {
    Unchanged {
        digest: Digest,
    },
    Updated {
        digest: Digest,
        previous: Option<Digest>,
    },
}

pub struct RolloutCoordinator<D> {
    driver: D,
    current: Option<Digest>,
    poll_interval: Duration,
}

impl<D> RolloutCoordinator<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            current: None,
            poll_interval: Duration::from_secs(30),
        }
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval.max(Duration::from_secs(1));
        self
    }

    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }
}

impl<D: DeploymentDriver> RolloutCoordinator<D> {
    pub async fn reconcile(
        &mut self,
        spec: &DeploymentSpec,
        desired: Digest,
    ) -> Result<RolloutResult, AgentError> {
        spec.validate()?;
        if self.current.as_ref() == Some(&desired) {
            return Ok(RolloutResult::Unchanged { digest: desired });
        }
        let previous = self.current.clone();
        self.driver.pull_digest(spec, &desired).await?;
        self.driver.pin_digest(spec, &desired).await?;
        self.driver.restart(spec).await?;
        if !self.driver.healthy(spec).await? {
            if let Some(previous_digest) = previous.as_ref() {
                self.driver.pin_digest(spec, previous_digest).await?;
                self.driver.restart(spec).await?;
                if !self.driver.healthy(spec).await? {
                    return Err(AgentError::RollbackFailed(
                        "previous digest did not become healthy".to_owned(),
                    ));
                }
            }
            return Err(AgentError::HealthCheckFailed);
        }
        self.current = Some(desired.clone());
        Ok(RolloutResult::Updated {
            digest: desired,
            previous,
        })
    }
}

#[async_trait]
pub trait ManifestSource: Send + Sync {
    async fn digest_for_tag(&self, spec: &DeploymentSpec) -> Result<Digest, AgentError>;
}

pub async fn poll_once<S: ManifestSource, D: DeploymentDriver>(
    source: &S,
    coordinator: &mut RolloutCoordinator<D>,
    spec: &DeploymentSpec,
) -> Result<RolloutResult, AgentError> {
    let digest = source.digest_for_tag(spec).await?;
    coordinator.reconcile(spec, digest).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn spec() -> DeploymentSpec {
        DeploymentSpec {
            registry: "https://registry.example.com".to_owned(),
            repository: "team/app".to_owned(),
            tag: "production".to_owned(),
            compose_project: "my-app".to_owned(),
            compose_service: "web".to_owned(),
            working_directory: PathBuf::from("/srv/app"),
            robot_username: "robot".to_owned(),
            robot_secret: "secret".to_owned(),
            health_timeout_seconds: 90,
        }
    }

    #[derive(Default)]
    struct FakeDriver {
        calls: Mutex<Vec<String>>,
        healthy: Mutex<Vec<bool>>,
    }

    #[async_trait]
    impl DeploymentDriver for FakeDriver {
        async fn pull_digest(&self, _: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError> {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("pull {digest}"));
            Ok(())
        }

        async fn pin_digest(&self, _: &DeploymentSpec, digest: &Digest) -> Result<(), AgentError> {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("pin {digest}"));
            Ok(())
        }

        async fn restart(&self, _: &DeploymentSpec) -> Result<(), AgentError> {
            self.calls.lock().expect("calls").push("restart".to_owned());
            Ok(())
        }

        async fn healthy(&self, _: &DeploymentSpec) -> Result<bool, AgentError> {
            Ok(self.healthy.lock().expect("health").pop().unwrap_or(true))
        }
    }

    #[tokio::test]
    async fn rollout_pulls_by_digest_and_ignores_unchanged_tags() {
        let driver = FakeDriver::default();
        let mut coordinator = RolloutCoordinator::new(driver);
        let digest = Digest::sha256(b"release-1");
        let result = coordinator
            .reconcile(&spec(), digest.clone())
            .await
            .expect("rollout");
        assert_eq!(
            result,
            RolloutResult::Updated {
                digest: digest.clone(),
                previous: None
            }
        );
        assert_eq!(
            coordinator
                .reconcile(&spec(), digest.clone())
                .await
                .expect("unchanged"),
            RolloutResult::Unchanged { digest }
        );
    }

    #[test]
    fn configuration_rejects_command_injection_characters() {
        let mut config = spec();
        config.compose_service = "web; rm -rf /".to_owned();
        assert!(matches!(
            config.validate(),
            Err(AgentError::InvalidConfig("compose service"))
        ));
    }
}
