use motyga_app_server_protocol::ConfigLayer as ApiConfigLayer;
use motyga_app_server_protocol::ConfigLayerMetadata as ApiConfigLayerMetadata;
use motyga_app_server_protocol::ConfigLayerSource as ApiConfigLayerSource;
use motyga_config::ConfigLayer;
use motyga_config::ConfigLayerMetadata;
use motyga_config::ConfigLayerSource;

/// Converts a config-layer source owned by `motyga-config` into the app-server wire type owned by
/// `motyga-app-server-protocol`.
///
/// The types stay separate so app-server protocol ownership does not leak into the config domain
/// crate. Because this crate owns neither type, Rust's orphan rules require an explicit conversion
/// function instead of a `From` implementation.
pub(crate) fn config_layer_source_to_api(source: ConfigLayerSource) -> ApiConfigLayerSource {
    match source {
        ConfigLayerSource::Mdm { domain, key } => ApiConfigLayerSource::Mdm { domain, key },
        ConfigLayerSource::System { file } => ApiConfigLayerSource::System { file },
        ConfigLayerSource::EnterpriseManaged { id, name } => {
            ApiConfigLayerSource::EnterpriseManaged { id, name }
        }
        ConfigLayerSource::User { file, profile } => ApiConfigLayerSource::User { file, profile },
        ConfigLayerSource::Project { dot_motyga_folder } => {
            ApiConfigLayerSource::Project { dot_motyga_folder }
        }
        ConfigLayerSource::SessionFlags => ApiConfigLayerSource::SessionFlags,
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => {
            ApiConfigLayerSource::LegacyManagedConfigTomlFromFile { file }
        }
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => {
            ApiConfigLayerSource::LegacyManagedConfigTomlFromMdm
        }
    }
}

/// Converts config-layer metadata owned by `motyga-config` into the app-server wire type owned by
/// `motyga-app-server-protocol`.
///
/// The types stay separate so app-server protocol ownership does not leak into the config domain
/// crate. Because this crate owns neither type, Rust's orphan rules require an explicit conversion
/// function instead of a `From` implementation.
pub(crate) fn config_layer_metadata_to_api(
    metadata: ConfigLayerMetadata,
) -> ApiConfigLayerMetadata {
    ApiConfigLayerMetadata {
        name: config_layer_source_to_api(metadata.name),
        version: metadata.version,
    }
}

/// Converts a config layer owned by `motyga-config` into the app-server wire type owned by
/// `motyga-app-server-protocol`.
///
/// The types stay separate so app-server protocol ownership does not leak into the config domain
/// crate. Because this crate owns neither type, Rust's orphan rules require an explicit conversion
/// function instead of a `From` implementation.
pub(crate) fn config_layer_to_api(layer: ConfigLayer) -> ApiConfigLayer {
    ApiConfigLayer {
        name: config_layer_source_to_api(layer.name),
        version: layer.version,
        config: layer.config,
        disabled_reason: layer.disabled_reason,
    }
}
