-- Deployment-level fallback: chain to a different deployment (different model)
-- when all backends for the current deployment are exhausted.
ALTER TABLE model_deployments
    ADD COLUMN fallback_deployment_id UUID REFERENCES model_deployments(id);
