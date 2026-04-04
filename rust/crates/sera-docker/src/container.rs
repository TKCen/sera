//! Container lifecycle management via bollard.

use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::Docker;
use std::collections::HashMap;

use crate::error::DockerError;
use crate::ExecOutput;

/// Manages Docker container lifecycle for agent instances.
pub struct ContainerManager {
    docker: Docker,
}

impl ContainerManager {
    /// Create a new ContainerManager, connecting to the local Docker daemon.
    pub fn new() -> Result<Self, DockerError> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| DockerError::Connection(e.to_string()))?;
        Ok(Self { docker })
    }

    /// Create and start a container for an agent instance.
    pub async fn start_container(
        &self,
        instance_id: &str,
        instance_name: &str,
        template_name: &str,
        image: &str,
        network: &str,
        env_vars: HashMap<String, String>,
    ) -> Result<String, DockerError> {
        let container_name = format!("sera-agent-{}", &instance_id[..8]);

        let env: Vec<String> = env_vars
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let config = Config {
            image: Some(image.to_string()),
            hostname: Some(container_name.clone()),
            env: Some(env),
            labels: Some(HashMap::from([
                ("sera.agent.id".to_string(), instance_id.to_string()),
                ("sera.agent.name".to_string(), instance_name.to_string()),
                ("sera.template".to_string(), template_name.to_string()),
                ("sera.managed".to_string(), "true".to_string()),
            ])),
            host_config: Some(bollard::models::HostConfig {
                network_mode: Some(network.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        let response = self
            .docker
            .create_container(Some(opts), config)
            .await
            .map_err(|e| DockerError::Api(format!("Failed to create container: {e}")))?;

        self.docker
            .start_container(&response.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| DockerError::Api(format!("Failed to start container: {e}")))?;

        tracing::info!(
            container_id = %response.id,
            instance_id,
            "Started agent container"
        );

        Ok(response.id)
    }

    /// Stop and remove a container.
    pub async fn stop_container(&self, container_id: &str) -> Result<(), DockerError> {
        // Stop with 10s timeout
        let stop_opts = StopContainerOptions { t: 10 };
        if let Err(e) = self.docker.stop_container(container_id, Some(stop_opts)).await {
            tracing::warn!(container_id, "Stop failed (may already be stopped): {e}");
        }

        // Remove
        let remove_opts = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        self.docker
            .remove_container(container_id, Some(remove_opts))
            .await
            .map_err(|e| DockerError::Api(format!("Failed to remove container: {e}")))?;

        tracing::info!(container_id, "Stopped and removed agent container");
        Ok(())
    }

    /// Execute a command in a running container.
    pub async fn exec_in_container(
        &self,
        container_id: &str,
        command: &[String],
        working_dir: Option<&str>,
    ) -> Result<ExecOutput, DockerError> {
        use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
        use futures_util::stream::StreamExt;

        let exec_opts = CreateExecOptions {
            cmd: Some(command.to_vec()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            working_dir: working_dir.map(|s| s.to_string()),
            ..Default::default()
        };

        let exec_id = self
            .docker
            .create_exec(container_id, exec_opts)
            .await
            .map_err(|e| DockerError::Api(format!("Failed to create exec: {e}")))?;

        let mut output = ExecOutput {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        };

        let start_opts = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let result = self
            .docker
            .start_exec(&exec_id.id, Some(start_opts))
            .await
            .map_err(|e| DockerError::Api(format!("Failed to start exec: {e}")))?;

        // Collect stdout/stderr from the attached stream
        if let StartExecResults::Attached { output: mut stream, input: _ } = result {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(bollard::container::LogOutput::StdOut { message }) => {
                        output.stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    Ok(bollard::container::LogOutput::StdErr { message }) => {
                        output.stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Exec stream error: {e}");
                        break;
                    }
                }
            }
        }

        // Get exit code
        let inspect = self
            .docker
            .inspect_exec(&exec_id.id)
            .await
            .map_err(|e| DockerError::Api(format!("Failed to inspect exec: {e}")))?;

        if let Some(code) = inspect.exit_code {
            output.exit_code = code;
        }

        tracing::debug!(
            container_id,
            exit_code = output.exit_code,
            "Executed command in container"
        );

        Ok(output)
    }
}
