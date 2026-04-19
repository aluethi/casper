// ═══════════════════════════════════════════════════════════════════
// Casper Platform — Azure Infrastructure
// ═══════════════════════════════════════════════════════════════════
//
// Deploys: Container Registry, PostgreSQL 16 + pgvector,
//          Container Apps Environment, casper-server Container App
//
// Usage:
//   az group create -n rg-casper-dev -l switzerlandnorth
//   az deployment group create -g rg-casper-dev -f infra/main.bicep \
//     -p postgresPassword='<secure>' env=dev
//
// The GitHub Actions workflow (deploy.yml) automates this.

targetScope = 'resourceGroup'

// ── Parameters ──────────────────────────────────────────────────

@description('Azure region for all resources')
param location string = resourceGroup().location

@description('Environment name')
@allowed(['dev', 'staging', 'prod'])
param env string = 'dev'

@description('PostgreSQL administrator password')
@secure()
param postgresPassword string

@description('Vault master key (base64-encoded, 32 bytes). Empty = dev mode.')
@secure()
param masterKey string = ''

@description('Admin email for bootstrapped platform admin user')
param adminEmail string = 'admin@ventoo.ch'

@description('Full container image to deploy. Defaults to a quickstart image for initial deploy.')
param containerImage string = 'mcr.microsoft.com/k8se/quickstart:latest'

// ── Variables ───────────────────────────────────────────────────

var prefix = 'casper-${env}'
var uniqueSuffix = uniqueString(resourceGroup().id)
var acrName = 'acrcasper${uniqueSuffix}'
var isDevAuth = empty(masterKey)

// PostgreSQL
var pgServerName = 'psql-${prefix}'
var pgAdminUser = 'casperadmin'
var pgDbName = 'casper'
var pgConnStr = 'postgresql://${pgAdminUser}:${postgresPassword}@${pgServerName}.postgres.database.azure.com:5432/${pgDbName}?sslmode=require'

// ── Log Analytics ───────────────────────────────────────────────

resource logAnalytics 'Microsoft.OperationalInsights/workspaces@2023-09-01' = {
  name: 'log-${prefix}'
  location: location
  properties: {
    sku: { name: 'PerGB2018' }
    retentionInDays: 30
  }
}

// ── Container Registry ──────────────────────────────────────────

resource acr 'Microsoft.ContainerRegistry/registries@2023-11-01-preview' = {
  name: acrName
  location: location
  sku: { name: 'Basic' }
  properties: { adminUserEnabled: true }
}

// ── PostgreSQL Flexible Server ──────────────────────────────────

resource postgres 'Microsoft.DBforPostgreSQL/flexibleServers@2024-08-01' = {
  name: pgServerName
  location: location
  sku: {
    name: 'Standard_B1ms'
    tier: 'Burstable'
  }
  properties: {
    version: '16'
    administratorLogin: pgAdminUser
    administratorLoginPassword: postgresPassword
    storage: { storageSizeGB: 32 }
    backup: {
      backupRetentionDays: 7
      geoRedundantBackup: 'Disabled'
    }
    highAvailability: { mode: 'Disabled' }
  }
}

resource postgresDb 'Microsoft.DBforPostgreSQL/flexibleServers/databases@2024-08-01' = {
  parent: postgres
  name: pgDbName
}

// Enable pgvector extension
resource pgvectorExt 'Microsoft.DBforPostgreSQL/flexibleServers/configurations@2024-08-01' = {
  parent: postgres
  name: 'azure.extensions'
  properties: {
    value: 'VECTOR'
    source: 'user-override'
  }
}

// Allow Azure services (Container Apps) to reach PostgreSQL
resource pgFirewall 'Microsoft.DBforPostgreSQL/flexibleServers/firewallRules@2024-08-01' = {
  parent: postgres
  name: 'AllowAzureServices'
  properties: {
    startIpAddress: '0.0.0.0'
    endIpAddress: '0.0.0.0'
  }
}

// ── Container Apps Environment ──────────────────────────────────

resource cae 'Microsoft.App/managedEnvironments@2024-03-01' = {
  name: 'cae-${prefix}'
  location: location
  properties: {
    appLogsConfiguration: {
      destination: 'log-analytics'
      logAnalyticsConfiguration: {
        customerId: logAnalytics.properties.customerId
        sharedKey: logAnalytics.listKeys().primarySharedKey
      }
    }
  }
}

// ── Container App (casper-server) ───────────────────────────────

resource app 'Microsoft.App/containerApps@2024-03-01' = {
  name: 'ca-${prefix}'
  location: location
  properties: {
    managedEnvironmentId: cae.id
    configuration: {
      activeRevisionsMode: 'Single'
      ingress: {
        external: true
        targetPort: 3000
        transport: 'auto'
        allowInsecure: false
      }
      registries: [
        {
          server: acr.properties.loginServer
          username: acr.listCredentials().username
          passwordSecretRef: 'acr-password'
        }
      ]
      secrets: [
        { name: 'acr-password', value: acr.listCredentials().passwords[0].value }
        { name: 'database-url', value: pgConnStr }
        { name: 'master-key', value: isDevAuth ? 'unused' : masterKey }
      ]
    }
    template: {
      containers: [
        {
          name: 'casper-server'
          image: containerImage
          resources: {
            cpu: json('0.5')
            memory: '1Gi'
          }
          env: [
            { name: 'DATABASE_URL', secretRef: 'database-url' }
            { name: 'DATABASE_OWNER_URL', secretRef: 'database-url' }
            { name: 'CASPER_PUBLIC_URL', value: 'https://ca-${prefix}.${cae.properties.defaultDomain}' }
            { name: 'CASPER_DEV_AUTH', value: isDevAuth ? 'true' : 'false' }
            { name: 'CASPER_MASTER_KEY', secretRef: 'master-key' }
            { name: 'CASPER_ADMIN_EMAIL', value: adminEmail }
            { name: 'RUST_LOG', value: 'info,sqlx=warn' }
          ]
          probes: [
            {
              type: 'Liveness'
              httpGet: { path: '/health', port: 3000 }
              periodSeconds: 30
              failureThreshold: 3
            }
            {
              type: 'Readiness'
              httpGet: { path: '/health', port: 3000 }
              periodSeconds: 10
              initialDelaySeconds: 5
            }
          ]
        }
      ]
      scale: {
        minReplicas: 1
        maxReplicas: env == 'prod' ? 5 : 2
        rules: [
          {
            name: 'http-scaling'
            http: { metadata: { concurrentRequests: '50' } }
          }
        ]
      }
    }
  }
}

// ── Outputs ─────────────────────────────────────────────────────

output acrLoginServer string = acr.properties.loginServer
output acrName string = acr.name
output appFqdn string = app.properties.configuration.ingress.fqdn
output appUrl string = 'https://${app.properties.configuration.ingress.fqdn}'
output postgresHost string = '${postgres.name}.postgres.database.azure.com'
