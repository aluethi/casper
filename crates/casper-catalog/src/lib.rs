/// Model catalog, backends, quotas, deployments, and routing.
pub mod routing;

pub use routing::{
    ResolvedBackend, ResolvedDeployment, check_quota, merge_params, resolve_deployment,
};
