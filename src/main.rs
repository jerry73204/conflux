//! msync node entry point.

use eyre::{Result, WrapErr, bail};
use msync::{Config, MsyncNode};
use rclrs::{Context, CreateBasicExecutor, RclrsErrorFilter, SpinOptions};
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Initialize ROS2 context and executor
    let context = Context::default_from_env()
        .map_err(|e| eyre::eyre!("Failed to create ROS2 context: {}", e))?;
    let mut executor = context.create_basic_executor();

    // Create the node
    let node = executor
        .create_node("msync")
        .map_err(|e| eyre::eyre!("Failed to create ROS2 node: {}", e))?;

    // Get config file path from parameter
    let config_file: Arc<str> = node
        .declare_parameter::<Arc<str>>("config_file")
        .mandatory()
        .map_err(|e| eyre::eyre!("Missing required parameter 'config_file': {}", e))?
        .get();

    if config_file.is_empty() {
        bail!(
            "Parameter 'config_file' is required.\n\
             Usage: ros2 run msync msync --ros-args -p config_file:=/path/to/config.yaml"
        );
    }

    info!(config_file = %config_file, "Loading configuration");

    // Load and validate configuration
    let config = Config::load(config_file.as_ref())?;

    info!(
        num_inputs = config.inputs.len(),
        output_topic = %config.output.topic,
        window_size = ?config.sync.window_size,
        buffer_size = config.sync.buffer_size,
        "Configuration loaded"
    );

    // Create the msync node
    let msync_node = MsyncNode::new(node.clone(), config)?;

    // Create tokio runtime and run the node
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .wrap_err("Failed to create tokio runtime")?;

    // Spawn the synchronization task
    let sync_handle = runtime.spawn(async move {
        if let Err(e) = msync_node.run().await {
            error!(error = %e, "Synchronization task failed");
        }
    });

    info!("msync node started, spinning...");

    // Spin the ROS2 executor (this blocks and processes callbacks)
    // The sync task runs in the background on the tokio runtime
    executor
        .spin(SpinOptions::default())
        .first_error()
        .map_err(|e| eyre::eyre!("Error while spinning: {}", e))?;

    // Wait for sync task to complete
    runtime.block_on(async {
        let _ = sync_handle.await;
    });

    info!("msync node shutting down");
    Ok(())
}
