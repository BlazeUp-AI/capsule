//! Capsule Runtime - Docker container management

use bollard::container::{
    Config, CreateContainerOptions, DownloadFromContainerOptions, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tracing::info;

const DEFAULT_IMAGE: &str = "capsule-runtime:latest";

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker error: {0}")]
    Bollard(#[from] bollard::errors::Error),
    #[error("Container not found: {0}")]
    NotFound(String),
    #[error("Exec timed out after {0:?}")]
    ExecTimeout(Duration),
}

pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

pub struct ContainerConfig {
    pub image: Option<String>,
    pub env_vars: HashMap<String, String>,
    pub enable_dind: bool,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            image: None,
            env_vars: HashMap::new(),
            enable_dind: false,
        }
    }
}

pub struct ContainerManager {
    docker: Docker,
}

impl ContainerManager {
    pub fn new() -> Result<Self, DockerError> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    /// Create and start a new container for a session
    pub async fn create_container(
        &self,
        session_id: &str,
        config: &ContainerConfig,
    ) -> Result<String, DockerError> {
        let container_name = format!("capsule-{}", &session_id[..8]);
        let image = config.image.as_deref().unwrap_or(DEFAULT_IMAGE);

        let mut env: Vec<String> = vec![
            "TERM=xterm-256color".to_string(),
            "LANG=C.UTF-8".to_string(),
            "LC_ALL=C.UTF-8".to_string(),
        ];

        for (key, value) in &config.env_vars {
            env.push(format!("{key}={value}"));
        }

        let mut binds: Vec<String> = Vec::new();
        if config.enable_dind {
            binds.push("/var/run/docker.sock:/var/run/docker.sock".to_string());
        }

        let host_config = bollard::service::HostConfig {
            memory: Some(4 * 1024 * 1024 * 1024), // 4GB
            nano_cpus: Some(2_000_000_000),        // 2 CPUs
            pids_limit: Some(256),
            network_mode: Some("bridge".to_string()),
            binds: if binds.is_empty() { None } else { Some(binds) },
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(image.to_string()),
            hostname: Some("capsule".to_string()),
            tty: Some(true),
            open_stdin: Some(true),
            env: Some(env),
            working_dir: Some("/workspace".to_string()),
            user: Some("developer".to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        let response = self
            .docker
            .create_container(Some(options), container_config)
            .await?;
        let container_id = response.id;

        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await?;

        info!(container_id = %container_id, container_name = %container_name, image = %image, "Container created");
        Ok(container_id)
    }

    /// Execute a command in a container and return the output
    pub async fn exec_command(
        &self,
        container_id: &str,
        command: Vec<String>,
        timeout: Duration,
    ) -> Result<ExecResult, DockerError> {
        let exec_options = CreateExecOptions {
            cmd: Some(command),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            working_dir: Some("/workspace".to_string()),
            user: Some("developer".to_string()),
            ..Default::default()
        };

        let exec = self.docker.create_exec(container_id, exec_options).await?;

        let start_result = self.docker.start_exec(&exec.id, None).await?;

        let mut stdout = String::new();
        let mut stderr = String::new();

        if let StartExecResults::Attached { mut output, .. } = start_result {
            let collect = async {
                while let Some(Ok(msg)) = output.next().await {
                    match msg {
                        bollard::container::LogOutput::StdOut { message } => {
                            stdout.push_str(&String::from_utf8_lossy(&message));
                        }
                        bollard::container::LogOutput::StdErr { message } => {
                            stderr.push_str(&String::from_utf8_lossy(&message));
                        }
                        _ => {}
                    }
                }
            };

            if tokio::time::timeout(timeout, collect).await.is_err() {
                return Err(DockerError::ExecTimeout(timeout));
            }
        }

        let inspect = self.docker.inspect_exec(&exec.id).await?;
        let exit_code = inspect.exit_code.unwrap_or(-1);

        Ok(ExecResult {
            exit_code,
            stdout,
            stderr,
        })
    }

    /// Remove a container
    pub async fn remove_container(&self, container_id: &str) -> Result<(), DockerError> {
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await?;

        info!(container_id = %container_id, "Container removed");
        Ok(())
    }

    /// Get the Docker client for exec operations
    pub fn docker(&self) -> &Docker {
        &self.docker
    }

    /// Download workspace directory as tar archive
    pub async fn export_workspace(&self, container_id: &str) -> Result<Vec<u8>, DockerError> {
        let options = DownloadFromContainerOptions {
            path: "/workspace",
        };

        let mut stream = self
            .docker
            .download_from_container(container_id, Some(options));
        let mut data = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
        }

        info!(container_id = %container_id, size = data.len(), "Exported workspace");
        Ok(data)
    }
}
