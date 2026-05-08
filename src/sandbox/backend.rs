use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Context;

#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    fn backend_name(&self) -> &str;
    async fn create_sandbox(&self, task_id: usize) -> anyhow::Result<SandboxSession>;
    async fn destroy_sandbox(&self, task_id: usize) -> anyhow::Result<()>;
    async fn exec_command(&self, task_id: usize, cmd: Vec<&str>) -> anyhow::Result<String>;
    fn list_active(&self) -> Vec<usize>;
}

pub struct SandboxSession {
    pub task_id: usize,
    pub working_dir: String,
}

// ====================== Pty Backend ======================

#[derive(Clone)]
pub struct PtyBackend {
    sessions: Arc<Mutex<HashMap<usize, PtyInstance>>>,
}

struct PtyInstance {
    working_dir: PathBuf,
}

impl PtyBackend {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_working_dir(&self, task_id: usize) -> PathBuf {
        PathBuf::from(format!("/tmp/solo3_task_{}", task_id))
    }
}

impl Default for PtyBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SandboxBackend for PtyBackend {
    fn backend_name(&self) -> &str {
        "pty"
    }

    async fn create_sandbox(&self, task_id: usize) -> anyhow::Result<SandboxSession> {
        let work_dir = self.get_working_dir(task_id);
        std::fs::create_dir_all(&work_dir)
            .with_context(|| format!("Failed to create working dir: {}", work_dir.display()))?;

        let sessions = self.sessions.clone();
        let mut sessions_lock = sessions.lock().await;
        sessions_lock.insert(task_id, PtyInstance {
            working_dir: work_dir.clone(),
        });

        Ok(SandboxSession {
            task_id,
            working_dir: work_dir.to_string_lossy().to_string(),
        })
    }

    async fn destroy_sandbox(&self, task_id: usize) -> anyhow::Result<()> {
        let sessions = self.sessions.clone();
        let mut sessions_lock = sessions.lock().await;
        if let Some(instance) = sessions_lock.remove(&task_id) {
            eprintln!("[PtyBackend] Sandbox {} destroyed (kept dir: {:?})", task_id, instance.working_dir);
        }
        Ok(())
    }

    async fn exec_command(&self, task_id: usize, cmd: Vec<&str>) -> anyhow::Result<String> {
        let sessions = self.sessions.clone();
        let work_dir = {
            let mut sessions_lock = sessions.lock().await;
            if let Some(instance) = sessions_lock.get(&task_id) {
                instance.working_dir.clone()
            } else {
                let work_dir = self.get_working_dir(task_id);
                std::fs::create_dir_all(&work_dir)?;
                sessions_lock.insert(task_id, PtyInstance {
                    working_dir: work_dir.clone(),
                });
                work_dir
            }
        };

        let output = std::process::Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(&work_dir)
            .output()
            .with_context(|| format!("Failed to execute: {:?}", cmd))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !stderr.is_empty() && output.status.success() {
            Ok(stdout)
        } else if !stderr.is_empty() {
            Ok(format!("{}\n{}", stdout, stderr))
        } else {
            Ok(stdout)
        }
    }

    fn list_active(&self) -> Vec<usize> {
        // Note: This is sync but we use block_on for the async Mutex
        let sessions = futures::executor::block_on(self.sessions.lock());
        sessions.keys().cloned().collect()
    }
}

// ====================== Docker Backend ======================

#[cfg(feature = "sandbox")]
pub mod docker {
    use super::*;
    use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
    use bollard::Docker;
    use bollard::exec::{CreateExecOptions, StartExecResults};

    const SANDBOX_IMAGE: &str = "ubuntu:22.04";
    const SANDBOX_WORKSPACE_DIR: &str = "/sessions";

    pub struct DockerBackend {
        docker: Docker,
        instances: Arc<Mutex<HashMap<usize, DockerInstance>>>,
    }

    struct DockerInstance {
        container_id: String,
        task_id: usize,
        workspace_path: String,
    }

    impl Clone for DockerInstance {
        fn clone(&self) -> Self {
            Self {
                container_id: self.container_id.clone(),
                task_id: self.task_id,
                workspace_path: self.workspace_path.clone(),
            }
        }
    }

    impl DockerBackend {
        pub async fn new() -> anyhow::Result<Self> {
            let docker = Docker::connect_with_local_defaults()?;
            docker.ping().await?;

            Ok(Self {
                docker,
                instances: Arc::new(Mutex::new(HashMap::new())),
            })
        }

        async fn ensure_container(&self, task_id: usize) -> anyhow::Result<DockerInstance> {
            // Check existing
            {
                let instances = self.instances.lock().await;
                if let Some(inst) = instances.get(&task_id) {
                    return Ok(inst.clone());
                }
            }

            // Create new
            let workspace_path = format!("{}/{}/workspace", SANDBOX_WORKSPACE_DIR, task_id);
            let config = Config {
                image: Some(SANDBOX_IMAGE.to_string()),
                cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
                working_dir: Some("/".to_string()),
                env: Some(vec![format!("TASK_ID={}", task_id)]),
                host_config: Some(bollard::service::HostConfig {
                    auto_remove: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let container = self.docker
                .create_container(
                    Some(CreateContainerOptions {
                        name: format!("solo3-task-{}", task_id),
                        platform: None,
                    }),
                    config,
                )
                .await?;

            let instance = DockerInstance {
                container_id: container.id,
                task_id,
                workspace_path,
            };

            let mut instances = self.instances.lock().await;
            instances.insert(task_id, instance.clone());
            Ok(instance)
        }
    }

    #[async_trait::async_trait]
    impl SandboxBackend for DockerBackend {
        fn backend_name(&self) -> &str {
            "docker"
        }

        async fn create_sandbox(&self, task_id: usize) -> anyhow::Result<SandboxSession> {
            let instance = self.ensure_container(task_id).await?;
            Ok(SandboxSession {
                task_id,
                working_dir: instance.workspace_path,
            })
        }

        async fn destroy_sandbox(&self, task_id: usize) -> anyhow::Result<()> {
            let mut instances = self.instances.lock().await;
            if let Some(instance) = instances.remove(&task_id) {
                self.docker
                    .remove_container(
                        &instance.container_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await?;
            }
            Ok(())
        }

        async fn exec_command(&self, task_id: usize, cmd: Vec<&str>) -> anyhow::Result<String> {
            let instance = self.ensure_container(task_id).await?;

            let exec = self.docker.create_exec(
                &instance.container_id,
                CreateExecOptions {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(cmd),
                    ..Default::default()
                },
            )
            .await?;

            match self.docker.start_exec(&exec.id, None).await? {
                StartExecResults::Attached { mut output, .. } => {
                    use futures::StreamExt;
                    use bollard::container::LogOutput;
                    let mut result = Vec::new();
                    while let Some(msg) = futures::StreamExt::next(&mut output).await {
                        if let Ok(LogOutput::StdOut { message }) = msg {
                            result.extend_from_slice(&message);
                        } else if let Ok(LogOutput::StdErr { message }) = msg {
                            result.extend_from_slice(&message);
                        }
                    }
                    Ok(String::from_utf8_lossy(&result).to_string())
                }
                StartExecResults::Detached { .. } => Ok(String::new()),
            }
        }

        fn list_active(&self) -> Vec<usize> {
            // Note: This is sync but we use block_on for the async Mutex
            let instances = futures::executor::block_on(self.instances.lock());
            instances.keys().cloned().collect()
        }
    }
}

// ====================== Backend Detector ======================

pub enum Backend {
    Docker(Box<dyn SandboxBackend>),
    Pty(PtyBackend),
}

impl Clone for Backend {
    fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "sandbox")]
            Backend::Docker(_) => {
                // Docker backend can't be cloned easily, so we create a new one
                // This is a limitation - in practice, we'd want to reuse it
                Backend::Pty(PtyBackend::new())
            }
            #[cfg(not(feature = "sandbox"))]
            Backend::Docker(_) => Backend::Pty(PtyBackend::new()),
            Backend::Pty(b) => Backend::Pty(PtyBackend {
                sessions: b.sessions.clone(),
            }),
        }
    }
}

impl Backend {
    pub async fn detect() -> Self {
        #[cfg(feature = "sandbox")]
        {
            if let Ok(backend) = docker::DockerBackend::new().await {
                eprintln!("[Sandbox] Using Docker backend");
                return Backend::Docker(Box::new(backend));
            }
            eprintln!("[Sandbox] Docker not available, falling back to Pty");
        }

        #[cfg(not(feature = "sandbox"))]
        {
            eprintln!("[Sandbox] Docker support not compiled in, using Pty backend");
        }

        Backend::Pty(PtyBackend::new())
    }
}
