use std::sync::Arc;

use motyga_core::config::Config;
use motyga_extension_api::ConfigContributor;
use motyga_extension_api::ExtensionData;
use motyga_extension_api::ExtensionFuture;
use motyga_extension_api::ExtensionRegistryBuilder;
use motyga_extension_api::ThreadLifecycleContributor;
use motyga_extension_api::ThreadStartInput;
use motyga_extension_api::ToolCall;
use motyga_extension_api::ToolContributor;
use motyga_extension_api::ToolExecutor;
use motyga_login::AuthManager;
use motyga_model_provider::create_model_provider;
use motyga_model_provider_info::ModelProviderInfo;
use motyga_utils_absolute_path::AbsolutePathBuf;

use crate::backend::MotygaImagesBackend;
use crate::tool::ImageGenerationTool;

#[derive(Clone)]
struct ImageGenerationExtension {
    auth_manager: Arc<AuthManager>,
    resolve_save_root: Arc<SaveRootResolver>,
}

type SaveRootResolver = dyn Fn(&Config) -> Option<AbsolutePathBuf> + Send + Sync;

#[derive(Clone)]
struct ImageGenerationExtensionConfig {
    available: bool,
    provider: ModelProviderInfo,
    save_root: Option<AbsolutePathBuf>,
}

impl ImageGenerationExtensionConfig {
    /// Resolves the image provider and save root for a thread.
    fn from_config(config: &Config, resolve_save_root: &SaveRootResolver) -> Self {
        Self {
            available: config.model_provider.is_openai()
                || config.model_provider.uses_openai_actor_authorization(),
            provider: config.model_provider.clone(),
            save_root: resolve_save_root(config),
        }
    }
}

impl ThreadLifecycleContributor<Config> for ImageGenerationExtension {
    /// Seeds image-generation configuration when a thread begins.
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(ImageGenerationExtensionConfig::from_config(
                    input.config,
                    self.resolve_save_root.as_ref(),
                ));
        })
    }
}

impl ConfigContributor<Config> for ImageGenerationExtension {
    /// Refreshes image-generation configuration after thread configuration changes.
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(ImageGenerationExtensionConfig::from_config(
            new_config,
            self.resolve_save_root.as_ref(),
        ));
    }
}

impl ToolContributor for ImageGenerationExtension {
    /// Creates the image-generation tool exposed by this installed extension.
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        let Some(config) = thread_store.get::<ImageGenerationExtensionConfig>() else {
            return Vec::new();
        };
        if !config.available {
            return Vec::new();
        }

        vec![Arc::new(ImageGenerationTool::new(
            MotygaImagesBackend::new(create_model_provider(
                config.provider.clone(),
                Some(self.auth_manager.clone()),
            )),
            config.save_root.clone(),
            thread_store.level_id().to_string(),
        ))]
    }
}

/// Installs the standalone image-generation extension contributors.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    auth_manager: Arc<AuthManager>,
    resolve_save_root: impl Fn(&Config) -> Option<AbsolutePathBuf> + Send + Sync + 'static,
) {
    let extension = Arc::new(ImageGenerationExtension {
        auth_manager,
        resolve_save_root: Arc::new(resolve_save_root),
    });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.tool_contributor(extension);
}
