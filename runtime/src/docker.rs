//! Capsule Runtime - Docker container management

use bollard::container::{Config, CreateContainerOptions, DownloadFromContainerOptions, RemoveContainerOptions, StartContainerOptions};
use bollard::Docker;
use futures_util::StreamExt;
use thiserror::Error;
use tracing::info;

const IMAGE_NAME: &str = "capsule-runtime:latest";

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker error: {0}")]
    Bollard(#[from] bollard::errors::Error),
    #[error("Container not found: {0}")]
    NotFound(String),
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
    pub async fn create_container(&self, session_id: &str) -> Result<String, DockerError> {
        let container_name = format!("capsule-{}", &session_id[..8]);

        let config = Config {
            image: Some(IMAGE_NAME.to_string()),
            hostname: Some("capsule".to_string()),
            tty: Some(true),
            open_stdin: Some(true),
            env: Some(vec![
                "TERM=xterm-256color".to_string(),
                "LANG=C.UTF-8".to_string(),
                "LC_ALL=C.UTF-8".to_string(),
            ]),
            working_dir: Some("/workspace".to_string()),
            user: Some("developer".to_string()),
            host_config: Some(bollard::service::HostConfig {
                memory: Some(4 * 1024 * 1024 * 1024), // 4GB
                nano_cpus: Some(2_000_000_000),       // 2 CPUs
                pids_limit: Some(256),
                network_mode: Some("bridge".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        let response = self.docker.create_container(Some(options), config).await?;
        let container_id = response.id;

        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await?;

        info!(container_id = %container_id, container_name = %container_name, "Container created");
        Ok(container_id)
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

        let mut stream = self.docker.download_from_container(container_id, Some(options));
        let mut data = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
        }

        info!(container_id = %container_id, size = data.len(), "Exported workspace");
        Ok(data)
    }
}
