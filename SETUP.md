# Casper Platform — Azure Deployment

This guide walks through deploying Casper to a single Azure VM running
Docker Compose with automatic HTTPS via Caddy.

## Architecture

```
Internet
   │
   ▼  (443/HTTPS)
┌──────────────────────────── Azure VM (Standard_B2s) ──┐
│                                                        │
│  Caddy ──► casper-server ──► PostgreSQL 16 + pgvector  │
│         (reverse proxy)       (Docker volume)          │
│                    │                                   │
│                    ├──► SearXNG (web search)            │
│                    └──► /var/lib/casper/knowledge       │
└────────────────────────────────────────────────────────┘
         │
         ▼
   Azure Container Registry (Docker images)
```

Estimated cost: **~$35/month** (Standard_B2s VM + ACR Basic).

## Prerequisites

- [Azure CLI](https://learn.microsoft.com/en-us/cli/azure/install-azure-cli) (`az`)
- [GitHub CLI](https://cli.github.com/) (`gh`)
- An Azure subscription
- This repository pushed to GitHub

## 1. Azure Login

```bash
az login
az account set --subscription "<your-subscription-name-or-id>"
```

## 2. Create the Service Principal for GitHub OIDC

This lets GitHub Actions authenticate to Azure without storing passwords.

```bash
APP_ID=$(az ad app create --display-name "github-casper-deploy" --query appId -o tsv)
az ad sp create --id $APP_ID

SUB_ID=$(az account show --query id -o tsv)
az role assignment create \
  --assignee $APP_ID \
  --role Contributor \
  --scope /subscriptions/$SUB_ID

# Allow GitHub Actions on the main branch to assume this identity
az ad app federated-credential create --id $APP_ID --parameters '{
  "name": "github-main",
  "issuer": "https://token.actions.githubusercontent.com",
  "subject": "repo:aluethi/casper:ref:refs/heads/main",
  "audiences": ["api://AzureADTokenExchange"]
}'
```

> Replace `aluethi/casper` with your actual GitHub `owner/repo` if different.

## 3. Generate an SSH Key Pair

This key is used by the GitHub Actions workflow to deploy to the VM.

```bash
ssh-keygen -t ed25519 -f ~/.ssh/casper-deploy -N ""
```

## 4. Set GitHub Secrets

```bash
TENANT_ID=$(az account show --query tenantId -o tsv)
SUB_ID=$(az account show --query id -o tsv)

gh secret set AZURE_CLIENT_ID       --repo aluethi/casper --body "$APP_ID"
gh secret set AZURE_TENANT_ID       --repo aluethi/casper --body "$TENANT_ID"
gh secret set AZURE_SUBSCRIPTION_ID --repo aluethi/casper --body "$SUB_ID"
gh secret set VM_SSH_KEY            --repo aluethi/casper < ~/.ssh/casper-deploy
gh secret set VM_SSH_PUBLIC_KEY     --repo aluethi/casper < ~/.ssh/casper-deploy.pub
gh secret set POSTGRES_PASSWORD     --repo aluethi/casper  # will prompt for value
```

## 5. Deploy

Push to `main` and the workflow runs automatically:

```bash
git push origin main
```

Or trigger it manually:

```bash
gh workflow run deploy.yml
```

### What happens

1. **Bicep** provisions the Azure VM, networking, and Container Registry
2. **Docker image** is built and pushed to the registry
3. **Compose files** are copied to the VM via SCP
4. **SSH** writes the `.env`, pulls the new image, and starts the stack

First deploy takes ~10 minutes. Subsequent deploys (code changes only) take ~5 minutes.

## 6. Access

After deployment, the workflow summary shows the public URL. You can also find it with:

```bash
az network public-ip show \
  --resource-group rg-casper \
  --name pip-casper \
  --query dnsSettings.fqdn -o tsv
```

The URL will be something like `https://casper-xxxx.switzerlandnorth.cloudapp.azure.com`.

SSH into the VM:

```bash
ssh -i ~/.ssh/casper-deploy casperadmin@<fqdn>
```

## Operations

### View logs

```bash
ssh -i ~/.ssh/casper-deploy casperadmin@<fqdn>
cd /opt/casper
docker compose -f docker-compose.prod.yml logs -f app
```

### Restart services

```bash
docker compose -f docker-compose.prod.yml restart app
```

### Database backup

```bash
docker compose -f docker-compose.prod.yml exec db \
  pg_dump -U casper casper > backup-$(date +%F).sql
```

### Restore from backup

```bash
cat backup-2026-04-21.sql | docker compose -f docker-compose.prod.yml exec -T db \
  psql -U casper casper
```

### Update without CI

If you need to deploy a hotfix directly:

```bash
ssh -i ~/.ssh/casper-deploy casperadmin@<fqdn>
cd /opt/casper
docker compose -f docker-compose.prod.yml pull app
docker compose -f docker-compose.prod.yml up -d app
```

## Configuration

### Environment variables

These are set in `/opt/casper/.env` on the VM by the deploy workflow:

| Variable | Description |
|---|---|
| `POSTGRES_PASSWORD` | PostgreSQL password |
| `ACR_IMAGE` | Full image reference (registry/repo:tag) |
| `DOMAIN` | Public FQDN (used by Caddy for TLS) |
| `CASPER_DEV_AUTH` | `true` for dev mode (no real auth keys) |
| `CASPER_MASTER_KEY` | Base64-encoded 32-byte key (required when `DEV_AUTH=false`) |
| `CASPER_ADMIN_EMAIL` | Email for the bootstrapped admin user |

### Custom domain

1. Add a CNAME record pointing your domain to the Azure FQDN
2. Update `DOMAIN` in `.env` to your custom domain
3. Restart Caddy — it will auto-provision a certificate:

```bash
docker compose -f docker-compose.prod.yml restart caddy
```

### Production hardening

For a production deployment, set `CASPER_DEV_AUTH=false` and provide a real master key:

```bash
# Generate a 32-byte master key
openssl rand -base64 32
# → store as CASPER_MASTER_KEY in .env
```

You should also restrict the NSG SSH rule to your IP range instead of `*`.

## File layout

```
infra/
  main.bicep                  # Azure infrastructure (VM, ACR, networking)
  docker-compose.prod.yml     # Production Docker Compose stack
  Caddyfile                   # Reverse proxy config
.github/workflows/
  ci.yml                      # Build & test (PRs and pushes)
  deploy.yml                  # Build & deploy to Azure (main branch)
config/
  casper-server.yaml          # Local development config
  casper-server.docker.yaml   # Docker Compose config (baked into image)
Dockerfile                    # Multi-stage Rust build
```
